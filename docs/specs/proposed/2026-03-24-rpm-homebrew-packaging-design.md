# RPM & Homebrew Packaging for yoinkc

**Date:** 2026-03-24
**Status:** Proposed

## Summary

Package yoinkc as an RPM (for Fedora, RHEL 9, RHEL 10 / CentOS Stream) and
a Homebrew formula (for macOS), providing native `dnf install` and
`brew install` experiences alongside the existing container-based workflow.

## Motivation

Today yoinkc is distributed exclusively as a container image on GHCR.
Users run it via `run-yoinkc.sh`, which pulls the image and orchestrates
podman. This works but adds indirection:

- Container-in-container overhead for what is fundamentally a CLI tool
- No shell completion, no direct `--help` access
- macOS users running fleet/refine against collected tarballs still go
  through the container wrapper

Native packages remove this indirection while keeping the container workflow
available for users who prefer it.

## Product Boundary

The packaged CLI supports the full subcommand surface (`inspect`, `fleet`,
`refine`), but the primary use cases differ by platform:

- **RPM (Linux):** All three subcommands. `inspect` is the primary use case
  — sysadmins run it on the hosts they are migrating. Podman is a hard
  package dependency.
- **Homebrew (macOS):** `fleet` and `refine` are the primary use cases —
  processing collected tarballs on a workstation. `inspect` works if podman
  is installed and pointed at a Linux machine, but is an edge case. Podman
  is not a formula dependency; the `inspect` preflight checks for it at
  runtime and gives a clear install instruction if missing.

## Target Platforms

- **RPM:** Fedora (latest + rawhide), EPEL 9 (RHEL 9 / CentOS Stream 9),
  EPEL 10 (RHEL 10 / CentOS Stream 10)
- **Homebrew:** macOS (primary), Linux Homebrew (works but not a target)
- **RHEL 8:** Out of scope

## Versioning

- **Semver** (e.g., `0.2.0`, `1.0.0`)
- Version source of truth: `version` field in `pyproject.toml`
- RPM spec and Homebrew formula derive their version from this field

## Release Flow

### Two-tier model

- **Container (fast stream):** Every `v*` tag pushed to `main` triggers the
  existing `build-image.yml` workflow. Container image lands on GHCR with
  semver + SHA tags. No change to current behavior.
- **Packages (stable stream):** Creating a **published GitHub Release**
  triggers a new `package-release.yml` workflow that builds RPMs and updates
  the Homebrew formula. Not every tag needs a Release — the user chooses
  when to promote a tag.

### Release artifacts (attached to GitHub Release)

- Source tarball (GitHub auto-generates)
- `.src.rpm` (for COPR and manual rebuilds)
- `.rpm` for Fedora latest (convenience download)

## RPM Packaging

### Spec file

`yoinkc.spec` at repository root. Uses Fedora's `%pyproject_*` macros
(standard for Python RPM packaging on Fedora/EPEL).

### Dependencies

```
Requires: python3 >= 3.11
Requires: python3-pydantic >= 2.0
Requires: python3-jinja2 >= 3.1
Requires: podman
```

The `github` optional dependency group (PyGithub, GitPython) powers
`--push-to-github`, which is **not supported in packaged installs**. This
flag is a container-only feature. In packaged installs, `--push-to-github`
exits with an error stating it is unsupported and suggesting the equivalent
shell workflow (commit and push the output directory manually).

### Installed files

- `/usr/bin/yoinkc` — console script entry point
- `/usr/lib/python3.X/site-packages/yoinkc/` — package (including
  templates and static assets)
- `/usr/share/bash-completion/completions/yoinkc` — bash completion
- `/usr/share/zsh/site-functions/_yoinkc` — zsh completion
- `/usr/share/fish/vendor_completions.d/yoinkc.fish` — fish completion

No wrapper script, no systemd units, no config files.

### COPR

- Project: `marrusl/yoinkc`
- Build targets: Fedora latest + rawhide, EPEL 9, EPEL 10
- Auto-build from GitHub webhook on published releases
- User install: `dnf copr enable marrusl/yoinkc && dnf install yoinkc`

## Homebrew Packaging

### Tap repository

`marrusl/homebrew-yoinkc` on GitHub (separate repo). Contains a single
formula.

### Formula

`Formula/yoinkc.rb` — standard Python formula pattern:

- `depends_on "python@3.13"`
- `resource` blocks for PyPI dependencies: pydantic, pydantic-core, jinja2,
  markupsafe, annotated-types, typing-extensions, typing-inspection
- Installs into a Homebrew-managed virtualenv
- Podman is **not** a formula dependency — fleet and refine don't need it.
  The `inspect` preflight checks for podman at runtime.

### User experience

```
brew tap marrusl/yoinkc
brew install yoinkc
```

### macOS use cases

Primarily `yoinkc fleet` and `yoinkc refine` (processing collected
tarballs). `yoinkc inspect` works if pointed at a Linux podman machine but
is an edge case.

### Formula updates

The `package-release.yml` GHA workflow opens an automated PR against the
tap repo bumping the formula's `url` and `sha256` on published releases.
The PR-based approach allows review before the formula goes live.

## GHA Workflow: `package-release.yml`

### Trigger

```yaml
on:
  release:
    types: [published]
```

### Jobs

**`build-rpm`** (runs on `ubuntu-latest` with Fedora container):
- Checks out source at the release tag
- Builds SRPM using `%pyproject_*` macros
- Builds binary RPM for verification
- Uploads `.src.rpm` and `.rpm` as GitHub Release assets
COPR auto-builds are configured via webhook — COPR watches for published
GitHub Releases and triggers builds automatically. The GHA workflow does
not need to call the COPR API.

**`update-homebrew`** (runs on `ubuntu-latest`):
- Computes SHA256 of the GitHub-generated source tarball
- Checks out the `homebrew-yoinkc` tap repo
- Updates `url` and `sha256` in the formula
- Regenerates `resource` blocks from `pyproject.toml` dependencies
  (using `brew update-python-resources` or equivalent scripting) so that
  added, removed, or bumped Python dependencies are reflected in the formula
- Opens a PR against the tap repo

### Secrets

- `HOMEBREW_TAP_TOKEN` — PAT with repo scope on the tap repo

### Relationship to existing workflow

`build-image.yml` stays as-is, triggered by `push: tags: ["v*"]`. The two
workflows are independent — every release is also a tag push, so both fire.
Container image always updates; packages only update on published releases.

## CLI Changes

### Internalize wrapper logic

The following logic moves from `run-yoinkc.sh` into Python code:

**Podman preflight** (in `preflight.py` or similar):
- `inspect` subcommand checks `podman` is on PATH before proceeding
- Clear error: "yoinkc requires podman. Install it with:
  dnf install podman / brew install podman"

**Registry login check:**
- Only for `inspect` subcommand — inspect pulls bootc base images from
  registry.redhat.io to compute package deltas, requiring authentication.
  Fleet and refine operate on already-collected tarballs and need no
  registry access.
- Checks `podman login --get-login registry.redhat.io`
- Interactive terminal: prompts `podman login registry.redhat.io`
- Non-interactive: prints error with instructions and exits

### Wrapper script

`run-yoinkc.sh` continues to exist for the container-based workflow.
No changes to the wrapper itself. Users who don't install the RPM or
Homebrew formula use it exactly as before.

## Shell Completions

Hand-written static completion scripts committed to `completions/` in the
repository. The CLI surface is small (three subcommands with a handful of
flags each), so static scripts are easy to maintain and avoid adding a
dependency on argcomplete or similar libraries.

- `completions/yoinkc.bash` — bash completion
- `completions/yoinkc.zsh` — zsh completion
- `completions/yoinkc.fish` — fish completion

All three cover the subcommands (`inspect`, `fleet`, `refine`) and their
flags.

**Drift prevention:** A CI check in the test suite parses the argparse
definitions and verifies that all subcommands and flags are present in the
completion scripts. This runs on every PR, so CLI changes that forget to
update completions fail CI.

**RPM install paths:**
- `/usr/share/bash-completion/completions/yoinkc`
- `/usr/share/zsh/site-functions/_yoinkc`
- `/usr/share/fish/vendor_completions.d/yoinkc.fish`

**Homebrew:** Uses `bash_completion.install`, `zsh_completion.install`,
and `fish_completion.install` blocks in the formula.

## What This Does NOT Change

- Container image build and distribution (GHCR) — unchanged
- `run-yoinkc.sh` wrapper — unchanged, remains available
- `pyproject.toml` structure — unchanged (RPM spec reads from it)
- Python source code — unchanged except for the preflight additions
- Fleet and refine subcommand behavior — unchanged
