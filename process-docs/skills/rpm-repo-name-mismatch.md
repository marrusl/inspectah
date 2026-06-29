# RPM Repo Name Mismatch

## Problem

RHEL systems use two different naming schemes for the same repository:

- **Install-time short names** from `dnf repoquery --installed --queryformat "%{from_repo}"`:
  `AppStream`, `baseos`, `BaseOS`, `anaconda`
- **Full repo IDs** from `dnf repolist --enabled` and `.repo` file `[section]` headers:
  `rhel-9-for-aarch64-appstream-rpms`, `rhel-9-for-x86_64-baseos-rpms`

Exact string comparison between these two schemes always fails on real
RHEL systems. This caused ~50% false-positive repo-less flagging.

## Solution

Case-insensitive substring matching: lowercase the short name and check
if it appears anywhere in the lowercased full repo ID.

Two callsites implement this:

1. **`repoless.rs:repo_matches_enabled()`** — determines if a package's
   `source_repo` matches any enabled repo (repo-less detection).
2. **`source_repos.rs:normalize_source_repos()`** — rewrites `source_repo`
   from short name to full ID so package tables and config trees use the
   same identifier.

## Edge case: anaconda

`anaconda` is NOT a real repo. It is the install-time source recorded by
the Anaconda installer. No RHEL repo ID contains "anaconda", so substring
matching correctly leaves it unmatched. Packages from `anaconda` are
properly flagged as repo-less.

## Method constant registry

Non-RPM detection methods are centralized in `crates/core/src/util.rs`.
Every module that routes on `NonRpmItem.method` must use these constants:

| Constant | Value | Meaning |
|----------|-------|---------|
| `METHOD_PYTHON_VENV` | `"python venv"` | pyvenv.cfg-based venv |
| `METHOD_PIP_DIST_INFO` | `"pip dist-info"` | System pip via dist-info |
| `METHOD_NPM_LOCKFILE` | `"npm lockfile"` | package-lock.json |
| `METHOD_NPM_MANIFEST` | `"npm manifest"` | package.json only (no lockfile) |
| `METHOD_GEM_LOCKFILE` | `"gem lockfile"` | Gemfile.lock |
| `METHOD_GEM_SYSTEM` | `"gem system"` | `gem list --local` |

The `dedup_ecosystem()` function in `nonrpm.rs` maps these to ecosystem
buckets. New method constants must be added to both `util.rs` and
`dedup_ecosystem()`, plus the renderer's `is_language_env()` gate in
`language_packages.rs`.
