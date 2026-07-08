# Language Package Detection v2

**Status:** Proposed
**Date:** 2026-07-08

---

## Summary

Improve inspectah's language package detection fidelity and coverage based on
Collins's gap analysis of the non-RPM replication feature (shipped in Plans
1-4). This spec addresses the highest-impact gaps: missing npm global packages,
dead C-extension detection, limited scan roots, and renderer correctness issues.

**Scope:** Inspector, renderer, CLI, and refine UI changes. One plan, one SDD
run.

**Out of scope:** New ecosystem support (yarn, pnpm, poetry, conda, PHP,
maven). See Roadmap section.

---

## 1. npm Global Package Detection

### Problem

inspectah detects project-level npm packages (via `package-lock.json` or
`package.json` in scan roots) but does not detect globally installed npm
packages. A server with `npm install -g pm2` for process management loses that
tool after migration.

### Detection

Two independent methods, deduplicated:

1. **`npm list -g --json`** (preferred): When `npm` is on PATH, run
   `npm list -g --json` and parse the `dependencies` map for package names and
   exact installed versions. Confidence: **high**.

2. **Directory walk** (fallback): Walk `/usr/lib/node_modules` and
   `/usr/local/lib/node_modules`. Read `package.json` from each top-level
   module directory for name and version. Confidence: **medium**.

When both methods fire, prefer `npm list -g` output. Deduplicate by package
name.

### RPM Filtering

Same pattern as system pip: `rpm -qf /usr/lib/node_modules/<pkg>` to filter
RPM-owned packages (e.g., `nodejs-docs`). Remaining packages are
user-installed globals.

### Rendering

npm globals render as **active** (uncommented) and **unpinned** by default:

```dockerfile
# npm global packages (detected via npm list -g)
RUN npm install -g pm2 typescript
```

### Version Pinning

Detected versions are captured in the snapshot and displayed in the report UI
but not rendered into the Containerfile by default. Rationale: sysadmins
typically run `npm install -g pm2` without specifying a version — pinning to
the detected version freezes a version the user never intentionally chose.

**Refine UI toggle:** Per-package version pin toggle. Each detected global
package shows its detected version and a toggle to pin it. A per-ecosystem
override ("pin all npm globals") flips all toggles at once; individual packages
can be adjusted after the bulk flip.

When pinning is enabled:

```dockerfile
RUN npm install -g pm2@5.3.0 typescript@5.4.2
```

### Runtime Check

Same as project-level npm: warn if `nodejs` is not in the RPM package list.

### Method Constant

Add `METHOD_NPM_GLOBAL` (e.g., `"npm global"`) to `crates/core/src/util.rs`.

---

## 2. C-Extension Detection for pip

### Problem

The renderer has a C-extension warning gate (`has_c_extensions` on
`NonRpmItem`), but the inspector never sets the field to `true`. The detection
logic is absent — a dead feature gate. Users hit opaque build failures when the
generated Containerfile tries to `pip install` a package with C extensions
without build tools present.

### Detection

After inventorying a pip environment's packages, scan each package's directory
in `site-packages/<pkg>/` for `.so` files (compiled shared objects). If any
`.so` file is found, set `has_c_extensions: true` on the environment's
`NonRpmItem`.

Scope: Only scan discovered `site-packages` directories — no new filesystem
walks. This piggybacks on the existing pip detection pass.

### Rendering

Already implemented. When `has_c_extensions` is true, the renderer emits a
warning that build tools (`gcc`, `python3-devel`, `make`) may be needed for
`pip install` to succeed.

### UI

Show a "C extensions" badge on affected environments in the language package
list. Advisory only — no toggle needed.

---

## 3. Scan Expansion

### Problem

inspectah's hardcoded scan roots (`/opt`, `/srv`, `/usr/local` for project-level
language packages; `/usr/lib*` for system packages) miss common deployment
locations:

- `/home/*/` — `pip install --user` packages, nvm-managed Node.js,
  rbenv/rvm Ruby, application venvs under service accounts
- `/var/www/` — web application deployments (Django, Rails, Node.js, PHP)
- Custom paths (`/data/apps`, `/var/lib/myapp`)

This is the single largest coverage gap identified in the analysis.

### Updated Default Scan Roots

Add `/var/www` to the default scan roots. Full list:

- `/opt`
- `/srv`
- `/usr/local`
- `/var/www` **(new)**

Rationale: `/var/www` is a near-universal web server convention on RHEL. If
something is deployed there, the user wants it in scope. If the directory is
empty or absent, there is no cost.

### `--scan-home` Flag

Adds user home directories as scan roots by resolving actual paths from the
system's user database.

**Syntax:**

- `--scan-home all` — resolve home directories for all users with UID >= 1000
  via `getent passwd`. Scan each resolved directory.
- `--scan-home user1,user2` — resolve home directories for the named users via
  `getent passwd user1 user2`. Scan only those directories.
- Bare `--scan-home` (no argument) — **error** with a helpful message:
  `"--scan-home requires 'all' or a comma-separated user list (e.g., --scan-home all, --scan-home deploy,appuser)"`

**Behavior:**

- Home directories are resolved from `getent passwd` (catches LDAP/SSSD users),
  not hardcoded to `/home/<user>`.
- System users (UID < 1000) are skipped for `--scan-home all`. Explicitly named
  system users (e.g., `--scan-home nginx`) are included — the user made a
  conscious choice.
- If a named user does not exist, emit a warning and continue. Do not fail the
  scan.
- Duplicate suppression: if a resolved home directory falls under an existing
  scan root (e.g., `/opt/appuser`), do not scan it twice.

### `--scan-path` Flag

Adds arbitrary paths to the language package scan roots.

**Syntax:** Repeatable flag.

```
inspectah scan --scan-path /var/lib/myapp --scan-path /data/apps
```

**Behavior:**

- Validates each path exists at scan time. If not found, emit a warning and
  skip (do not fail the scan).
- Same walk behavior as existing scan roots — no depth limit.
- Duplicate suppression: if the path matches or is a subdirectory of an
  existing scan root, do not scan it twice.
- **Broad path warning:** If the path has fewer than two path components (e.g.,
  `/`, `/var`, `/usr`, `/home`), emit a stderr warning:
  `"Warning: --scan-path /<path> is very broad — this may be slow. Consider
  more specific paths (e.g., /var/www, /home/deploy)."`
  Do not block — let the user proceed if they choose.

### Composition

Both flags are additive on top of defaults. They compose naturally:

```
inspectah scan --scan-home deploy --scan-path /data/apps
```

This scans: `/opt` + `/srv` + `/usr/local` + `/var/www` (defaults) + deploy's
home directory + `/data/apps`.

### Discoverability

Print the effective scan root list in the scan output header so users can see
exactly what was scanned:

```
Scan roots: /opt, /srv, /usr/local, /var/www, /home/deploy, /data/apps
```

### Help Text

```
--scan-home <all|USER,...>   Scan user home directories for language packages.
                             'all' scans all users (UID >= 1000).
                             Comma-separated list scans specific users.
                             Home directories resolved via getent passwd.

--scan-path <PATH>           Add a path to the language package scan.
                             Repeatable. Additive with default scan roots.
```

---

## 4. `system_site_packages` Rendering Fix

### Problem

When a Python venv has `include-system-site-packages = true` in its
`pyvenv.cfg`, the recreated venv must have the same setting. The inspector
already captures this field, but the renderer ignores it. The generated
Containerfile creates a plain venv, breaking imports that relied on system
packages.

### Fix

When `system_site_packages` is `true` on the `NonRpmItem`, add
`--system-site-packages` to the `python3 -m venv` command:

```dockerfile
RUN python3 -m venv --system-site-packages /opt/myapp/venv \
    && /opt/myapp/venv/bin/pip install -r /tmp/venv-requirements.txt
```

### UI

Show a "system site-packages" indicator on affected venvs. Informational — no
toggle.

### Scope

Renderer-only change. The inspector already captures the field correctly.

---

## 5. Bundler Deprecation Fix

### Problem

The gem renderer uses `bundle install --deployment`, which is deprecated in
Bundler 2.1+. The flag still works in current versions but will eventually be
removed.

### Fix

Replace:

```dockerfile
RUN cd /opt/myapp && bundle install --deployment
```

With:

```dockerfile
RUN cd /opt/myapp && bundle config set --local deployment 'true' && bundle install
```

### Scope

Renderer-only, one-liner change.

---

## 6. Driftify Coverage

Extend driftify's `nonrpm` profile to generate test fixtures for new detection
capabilities:

| Feature | Fixture |
|---------|---------|
| npm globals | Packages in `/usr/lib/node_modules/` and `/usr/local/lib/node_modules/` with `package.json` files. Optionally make `npm` available for `npm list -g`. |
| C-extension packages | `.so` files inside `site-packages/<pkg>/` directories in existing pip venv fixtures. |
| `--scan-home` paths | Language environments (venvs, node projects, gem projects) under user home directories. Create test users with home dirs resolvable via `getent passwd`. |
| `/var/www` deployments | Language environments under `/var/www/` (Django venv, Node.js project). |
| `system_site_packages` | Existing venv fixtures with `include-system-site-packages = true` in `pyvenv.cfg`. |

Same fixture generation pattern as current pip/npm/gem profiles.

---

## 7. Roadmap (Deferred — Demand-Driven)

These items are documented for future consideration. Each follows the same
lockfile-parse-then-render pattern and can be specced independently when demand
warrants.

| Priority | Item | Trigger |
|----------|------|---------|
| P2 | **yarn.lock** parsing + `yarn install --frozen-lockfile --production` rendering | Customer demand or yarn project detected in field usage |
| P2 | **pnpm-lock.yaml** parsing + `pnpm install --frozen-lockfile --prod` rendering | Customer demand |
| P2 | **poetry.lock** detection + rendering (via `poetry install` or export to requirements.txt) | Customer demand or poetry adoption on RHEL targets |
| P3 | **Conda/mamba** environment detection (`conda-meta/`, `environment.yml`) | Data science workload migration demand |
| P3 | **PHP/Composer** detection (`composer.lock` / `composer.json`) | Web application migration demand |
| P3 | **Maven/Gradle** advisory-only detection (`pom.xml`, `build.gradle`) | Enterprise Java migration demand |
| — | **Version pinning for system pip/gems** — same per-package pin toggle as npm globals | Consistency follow-up |

### Adoption Context (as of 2026-07)

- **yarn:** 21.5% Node.js usage (declining), low-medium on RHEL migration targets, not in RHEL repos
- **pnpm:** 19.9% usage (rising fast, 92% retention), low on current migration targets, not in RHEL repos
- **poetry:** ~85M monthly PyPI downloads (+22% YoY), low-medium on RHEL targets, not in RHEL repos

None ship as RPMs in RHEL. All require third-party installation. Current RHEL
migration targets (3-7+ year old servers) predate mainstream adoption of pnpm
and poetry. yarn has the strongest legacy presence.

---

## Code Reference

| Component | Path |
|-----------|------|
| NonRpm inspector | `crates/collect/src/inspectors/nonrpm.rs` |
| NonRpmItem type | `crates/core/src/types/nonrpm.rs` |
| Language package renderer | `crates/pipeline/src/render/language_packages.rs` |
| Method constants | `crates/core/src/util.rs` |
| Refine UI — language packages | `crates/web/ui/src/components/LanguagePackageList.tsx` |
| TypeScript types | `crates/web/ui/src/api/types.ts` |
| driftify nonrpm profile | `src/profiles/nonrpm.rs` (in driftify repo) |
