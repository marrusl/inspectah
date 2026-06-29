# Language Package vs Unmanaged File Exclusion

> **Status:** Pre-spec (P0 bug)
> **Priority:** P0 — produces incorrect Containerfile output
> **Related:** Non-RPM replication (Tier 1 + Tier 2 interaction)

## Problem

When a language package environment (pip, npm, gem) is detected by the
Tier 1 scanner, AND the same files fall under Tier 2 unmanaged file
scan roots (`/opt`, `/srv`, `/usr/local`), the Containerfile renders
BOTH:

- A proper `RUN gem install bundler sinatra ...` (from Tier 1)
- 60+ `COPY unmanaged/usr/local/share/gems/...` lines (from Tier 2)

The COPY approach is wrong — it recreates the gem/pip/npm tree
file-by-file instead of using the native package manager. The
Containerfile is bloated, fragile, and won't get dependency resolution
or native extensions right.

## Root Cause

The unmanaged file scanner (`scan_unmanaged_files`) and the language
package scanner (`scan_gem_packages`, `scan_pip_packages`, etc.) run
independently. Neither knows what the other found. There is no
exclusion gate that says "don't treat these paths as unmanaged — they
belong to a detected language environment."

## Observed Impact

On a driftify `kitchen-sink` profile (RHEL 10, aarch64):
- System gems (bundler, sinatra, rack, etc.) installed via `gem install`
- Gem tree at `/usr/local/share/gems/gems/` detected as unmanaged files
- Containerfile has ~60 COPY lines for gem internals that should be
  a single `RUN gem install` line
- Same pattern would apply to pip packages in `/usr/local/lib/python3.*/`
  and npm packages in `/usr/local/lib/node_modules/`

## Proposed Fix

### Scanner-level exclusion

After language package detection completes, build a set of "owned paths"
— root directories of detected language environments. Pass this set to
the unmanaged file scanner as an exclusion list.

Example: if `scan_gem_packages` detects gems at
`/usr/local/share/gems/`, then `/usr/local/share/gems/` is excluded
from unmanaged scanning.

Candidate owned paths by ecosystem:
- **pip venvs:** the venv root (e.g., `/opt/myapp/venv/`)
- **pip system:** `/usr/lib/python3.*/site-packages/`,
  `/usr/local/lib/python3.*/site-packages/`
- **npm:** the project root containing `package.json` or
  `package-lock.json`
- **gem lockfile:** the project root containing `Gemfile.lock`
- **gem system:** `/usr/local/share/gems/`, `/usr/local/lib64/gems/`

### Renderer-level fallback

As a safety net, even if both a language package item and unmanaged
file items exist for the same path, the Containerfile renderer should
prefer the language-native approach (`RUN gem install`, `RUN pip
install`, etc.) and suppress the COPY lines for paths under that
environment.

## Open Questions

1. Should the exclusion be exact-prefix (exclude everything under
   `/usr/local/share/gems/`) or path-by-path (exclude each individual
   file the language scanner found)?
2. What about mixed scenarios — a venv at `/opt/myapp/venv/` plus
   non-Python files at `/opt/myapp/config/`? The venv subtree should
   be excluded but the config files should remain unmanaged.
3. Should the exclusion happen at scan time (don't collect the files)
   or at render time (collect but don't render)? Scan-time is cleaner
   but means the data isn't in the tarball for inspection.

## Scope

- Collector: add exclusion set from language packages to unmanaged scanner
- Renderer: add fallback suppression for overlapping paths
- No CLI changes
- No new scan flags
- Affects Containerfile output quality directly
