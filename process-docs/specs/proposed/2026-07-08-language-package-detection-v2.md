# Language Package Detection v2

**Status:** Proposed (R2)
**Date:** 2026-07-08

---

## Summary

Improve inspectah's language package detection fidelity and coverage based on a
gap analysis of the non-RPM replication feature (shipped in Plans 1-4). This
spec addresses the highest-impact gaps: missing npm global packages, dead
C-extension detection, limited scan roots, and renderer correctness issues.

**Scope:** Inspector, renderer, CLI, refine session/UI, and snapshot schema
changes. One plan, one SDD run.

**Out of scope:** New ecosystem support (yarn, pnpm, poetry, conda, PHP,
maven). See Roadmap section.

**Schema impact:** This spec adds an optional field to `InspectionSnapshot.meta`
and extends `LanguagePackage`, `LanguagePackageEnvDto`, `ItemId`, and
`RefinementOp`. Schema version bump required.

---

## 1. npm Global Package Detection

### Problem

inspectah detects project-level npm packages (via `package-lock.json` or
`package.json` in scan roots) but does not detect globally installed npm
packages. A server with `npm install -g pm2` for process management loses that
tool after migration.

### Detection

Two independent methods, merged per-package:

1. **`npm list -g --json`** (preferred): When `npm` is on PATH, run
   `npm list -g --json` and parse the `dependencies` map for package names and
   exact installed versions. Also run `npm root -g` to discover the actual
   global prefix path (do not assume `/usr/lib/node_modules`).
   Confidence: **high**.

2. **Directory walk** (fallback): Discover global prefix directories by
   checking both `npm root -g` output (if available) and the well-known
   fallback paths `/usr/lib/node_modules` and `/usr/local/lib/node_modules`.
   Walk each prefix for packages:
   - **Unscoped packages:** Read `<prefix>/<pkg>/package.json` for name and
     version.
   - **Scoped packages:** Entries starting with `@` are scope directories.
     Walk `<prefix>/@scope/<pkg>/package.json` for each sub-entry.
   Confidence: **medium**.

**Per-package merge rule:** When both methods produce results, merge per
package name. For each package, prefer the `npm list -g` entry (it has the
authoritative version). A package found only by directory walk is included at
medium confidence. A package found only by `npm list -g` is included at high
confidence (the filesystem path is inferred from `npm root -g` + package name).

**When `npm` is not on PATH:** Skip `npm list -g` and `npm root -g`. Fall back
to directory walk of the well-known paths only. All results are medium
confidence.

### Identity Model

npm globals use a single logical `NonRpmItem` per discovered global prefix
(the path from `npm root -g`, or the well-known fallback path). The
environment identity is:

- `ecosystem`: `"npm"`
- `method`: `"npm global"` (new `METHOD_NPM_GLOBAL` constant)
- `path`: the resolved global prefix (e.g., `/usr/lib/node_modules`)

If multiple prefixes contain global packages (e.g., both `/usr/lib/node_modules`
and `/usr/local/lib/node_modules`), each produces a separate `NonRpmItem`.
No cross-prefix deduplication — distinct prefixes are distinct environments,
same as distinct venv paths.

### RPM Filtering

For each discovered package, run `rpm -qf <prefix>/<pkg>` (or
`<prefix>/@scope/<pkg>` for scoped packages) to check RPM ownership. Filter
out RPM-owned packages. Same pattern as system pip.

### Rendering

npm globals render as **active** (uncommented) and **unpinned** by default:

```dockerfile
# npm global packages: /usr/lib/node_modules (detected via npm list -g)
RUN npm install -g pm2 typescript @angular/cli
```

### Version Pinning

Detected versions are captured in the snapshot and displayed in the report UI
but not rendered into the Containerfile by default. Rationale: sysadmins
typically run `npm install -g pm2` without specifying a version — pinning to
the detected version freezes a version the user never intentionally chose.

**Per-package pin state** is persisted through the refine session. See
Section 1a for the data model and interaction contract.

When pinning is enabled for a package:

```dockerfile
RUN npm install -g pm2@5.3.0 typescript@5.4.2 @angular/cli@17.3.0
```

### Runtime Check

Same as project-level npm: warn if `nodejs` is not in the RPM package list.

### Method Constant

Add `METHOD_NPM_GLOBAL` (e.g., `"npm global"`) to `crates/core/src/util.rs`.

---

## 1a. Per-Package Pin State — Data Model and Refine Contract

### Problem

The current refine model is environment-level: `LanguagePackageEnvDto` exposes
`packages: Vec<String>` and one `include` flag per environment.
`RefinementOp::SetInclude` with `ItemId::LanguageEnv { ecosystem, path }`
toggles inclusion. This is sufficient for "include/exclude this environment"
but not for per-package version pinning.

### Core Type Changes

**`LanguagePackage` (crates/core/src/types/nonrpm.rs):**

Add a `pinned` field:

```rust
pub struct LanguagePackage {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub pinned: bool,
}
```

Default is `false` (unpinned). The `version` field always carries the detected
version regardless of pin state — `pinned` controls whether the renderer uses
the version in the Containerfile output.

### Refine Contract Changes

**New `ItemId` variant:**

```rust
ItemId::LanguagePackage {
    ecosystem: String,   // e.g., "npm"
    env_path: String,    // e.g., "/usr/lib/node_modules"
    package: String,     // e.g., "pm2" or "@angular/cli"
}
```

**New `RefinementOp` variant:**

```rust
RefinementOp::SetPackagePin {
    item_id: ItemId,     // must be ItemId::LanguagePackage
    pinned: bool,
}
```

**Bulk pin operation:** `RefinementOp::SetBulkPackagePin` applies to all
packages within a `LanguageEnv`:

```rust
RefinementOp::SetBulkPackagePin {
    ecosystem: String,
    env_path: String,
    pinned: bool,
}
```

### DTO Changes

**`LanguagePackageEnvDto` (crates/web/src/web_types.rs):**

Replace `packages: Vec<String>` with a structured package list:

```rust
pub struct LanguagePackageDto {
    pub name: String,
    pub detected_version: String,
    pub pinned: bool,
}

pub struct LanguagePackageEnvDto {
    pub ecosystem: String,
    pub path: String,
    pub method: String,
    pub packages: Vec<LanguagePackageDto>,  // was Vec<String>
    pub confidence: String,
    pub manifest_basis: String,
    pub include: bool,
    pub has_c_extensions: bool,             // new, Section 2
    pub system_site_packages: bool,         // new, Section 4
}
```

### Session Persistence

Pin state is persisted in the refine session timeline via `SetPackagePin` and
`SetBulkPackagePin` ops. These are autosaved and replayed on session reload,
same as existing `SetInclude` ops.

**Export parity:** The renderer reads the projected `LanguagePackage.pinned`
field after session ops are applied. If `pinned` is `true`, the package is
rendered with `@version`; if `false`, without.

### Interaction Model

**Composition:** npm global environments render as expandable rows in the
language packages section. The environment row shows the path, method,
confidence, and an include/exclude toggle (existing pattern). Expanding the
row reveals a package sublist:

```
▸ npm globals: /usr/lib/node_modules (high confidence)     [include ✓]
    pm2          5.3.0     [pin ☐]
    typescript   5.4.2     [pin ☐]
    @angular/cli 17.3.0    [pin ☐]
  [Pin all]
```

**Bulk control:** A "Pin all" / "Unpin all" button at the bottom of the
package sublist. After bulk-pinning, if the user unpins one package, the bulk
button label changes to "Pin all" (not an indeterminate state — the button
always describes its next action, not its current state).

**Zero/one package edge cases:** If zero packages remain after RPM filtering,
the environment row shows "no user-installed packages" and the expand affordance
is absent. If one package exists, the sublist shows one row plus the bulk
button (bulk button still present for consistency — it acts as a single toggle).

**Search integration:** If `/` search or `Ctrl+K` matches a package name
(e.g., `pm2`), the matching environment row auto-expands and the matching
package row receives visual highlight. Clearing the search restores the prior
expand/collapse state.

### Keyboard and Accessibility Contract

**Pin toggle control:** Rendered as a checkbox (`<input type="checkbox">`).

**Tab sequence within an expanded environment:**
1. Environment row (include toggle)
2. First package pin checkbox
3. ... subsequent package pin checkboxes
4. "Pin all" button

**Key behavior:**
- `Space` on a pin checkbox toggles it
- `Enter` on the environment row toggles expand/collapse
- `ArrowDown`/`ArrowUp` move between package rows within the expanded sublist
- `Escape` within the sublist collapses the parent environment row; focus
  returns to the environment row

**Screen reader:**
- Pin checkbox: `aria-label="Pin pm2 to version 5.3.0"`
- Bulk button: `aria-label="Pin all packages in npm globals /usr/lib/node_modules"`
- Environment row: `aria-expanded="true|false"`, announced on toggle
- Bulk action feedback: live region announcement
  `"Pinned 3 packages in npm globals"` / `"Unpinned 3 packages in npm globals"`

**Badges (C extensions, system site-packages):** Rendered as static `<span>`
elements with `role="status"` and descriptive `aria-label` (e.g.,
`aria-label="This environment contains C extensions"`). Non-interactive — no
focus stop. Placed after the confidence indicator in the environment row.

---

## 2. C-Extension Detection for pip

### Problem

The renderer has a C-extension warning gate (`has_c_extensions` on
`NonRpmItem`), but the inspector never sets the field to `true`. The detection
logic is absent — a dead feature gate.

### Detection

**Environment-level rule:** After inventorying a pip environment, scan the
entire `site-packages/` directory tree recursively for any file matching
`*.so` (compiled shared objects). If any `.so` file is found anywhere in the
tree, set `has_c_extensions: true` on the environment's `NonRpmItem`.

This catches both layouts:
- Package subdirectory `.so` files: `site-packages/numpy/core/_multiarray.so`
- Top-level extension modules: `site-packages/ujson.cpython-311-x86_64-linux-gnu.so`

Per-package C-extension attribution (mapping `.so` files back to their owning
distribution via `RECORD` or `top_level.txt`) is out of scope for v2. The flag
is environment-level only.

### Rendering

Already implemented. When `has_c_extensions` is true, the renderer emits a
warning that build tools (`gcc`, `python3-devel`, `make`) may be needed.

### UI

Show a "C extensions" badge on affected environments (see Section 1a
accessibility contract for badge rendering).

---

## 3. Scan Expansion

### Problem

inspectah's hardcoded scan roots (`/opt`, `/srv`, `/usr/local`) miss common
deployment locations. This is the single largest coverage gap.

### Updated Default Scan Roots

Add `/var/www` to the default scan roots. Full list:

- `/opt`
- `/srv`
- `/usr/local`
- `/var/www` **(new)**

### Probe Scope

Expanded roots (`--scan-home`, `--scan-path`, and the new `/var/www` default)
are **full nonrpm roots** — they feed all probes, not just language package
detection. This includes ELF binary discovery, `.env` file collection, and git
repo detection.

Rationale: `--scan-path` and `--scan-home` are opt-in. The user explicitly
chose to scan these paths. Silently limiting to language-only probes would
violate least surprise — the flag is `--scan-path`, not
`--scan-path-for-languages`. Noise and sensitivity are handled by the existing
secrets redaction pipeline and refine UI filtering, not by restricting detection.

### `--scan-home` Flag

Adds user home directories as scan roots.

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
  system users (e.g., `--scan-home nginx`) are included.
- If a named user does not exist, emit a warning to stderr and continue.
- **User feedback for `all`:** Before scanning, emit to stderr the list of
  discovered users and their resolved home paths:
  `"--scan-home: scanning 4 users: deploy (/home/deploy), appuser (/opt/appuser), jenkins (/var/lib/jenkins), www (/var/www)"`
- Duplicate suppression: if a resolved home directory falls under an existing
  scan root (e.g., `/opt/appuser`), do not scan it twice.

### `--scan-path` Flag

Adds arbitrary paths as full scan roots.

**Syntax:** Repeatable flag.

```
inspectah scan --scan-path /var/lib/myapp --scan-path /data/apps
```

**Behavior:**

- Validates each path exists at scan time. If not found, emit a warning to
  stderr and skip.
- Same walk behavior as existing scan roots — no depth limit.
- Duplicate suppression: if the path matches or is a subdirectory of an
  existing scan root, do not scan it twice.
- **Broad path warning:** If the path has fewer than two path components (e.g.,
  `/`, `/var`, `/usr`, `/home`), emit a stderr warning:
  `"Warning: --scan-path /<path> is very broad — this may be slow. Consider
  more specific paths (e.g., /var/www, /home/deploy)."`
  Do not block.

### Composition

Both flags are additive on top of defaults. They compose naturally:

```
inspectah scan --scan-home deploy --scan-path /data/apps
```

Effective roots: `/opt` + `/srv` + `/usr/local` + `/var/www` (defaults) +
`/home/deploy` (from `--scan-home`) + `/data/apps` (from `--scan-path`).

### Output Channel Contract

**All scan-root messages go to stderr/progress stream. Never stdout.**

This preserves the existing `--inspect-only` contract where stdout is pure
JSON.

| Mode | Scan-root header | Warnings (missing user, missing path, broad path) |
|------|-----------------|---------------------------------------------------|
| Normal (default progress) | Shown on stderr | Shown on stderr |
| `--quiet` | Suppressed | Shown on stderr (warnings always visible) |
| `--progress flat` | Shown on stderr | Shown on stderr |
| `--inspect-only` | Suppressed | Shown on stderr |

### Scan Scope Persistence

The effective scan roots are persisted in `InspectionSnapshot.meta` under a
new key `"scan_roots"`:

```json
{
  "meta": {
    "schema_version": 22,
    "scan_roots": ["/opt", "/srv", "/usr/local", "/var/www", "/home/deploy"],
    "scan_home_users": ["deploy"],
    "scan_extra_paths": ["/data/apps"]
  }
}
```

- `scan_roots`: the full effective root list (defaults + expansions)
- `scan_home_users`: which users were resolved (empty if `--scan-home` not
  used, `["all"]` for `--scan-home all`)
- `scan_extra_paths`: paths added via `--scan-path` (empty if not used)

This enables downstream consumers (refine, reports, aggregate) to distinguish
"not found" from "not scanned." The report header and refine UI surface the
scan scope from this metadata.

### Help Text

```
--scan-home <all|USER,...>   Scan user home directories. 'all' scans all
                             users (UID >= 1000). Comma-separated list
                             scans specific users. Home directories
                             resolved via getent passwd. All probes run
                             (language packages, binaries, secrets, repos).

--scan-path <PATH>           Add a path to scan. Repeatable. Additive with
                             default scan roots. All probes run.
```

---

## 4. `system_site_packages` Rendering Fix

### Problem

When a Python venv has `include-system-site-packages = true` in its
`pyvenv.cfg`, the recreated venv must have the same setting. The inspector
already captures this field, but the renderer ignores it.

### Fix

When `system_site_packages` is `true` on the `NonRpmItem`, add
`--system-site-packages` to the `python3 -m venv` command:

```dockerfile
RUN python3 -m venv --system-site-packages /opt/myapp/venv \
    && /opt/myapp/venv/bin/pip install -r /tmp/venv-requirements.txt
```

This applies to all renderer branches that create venvs (high-confidence with
requirements.txt, medium-confidence with dist-info, system pip).

### UI

Show a "system site-packages" badge on affected venvs (see Section 1a
accessibility contract for badge rendering).

### Scope

Renderer-only change. The inspector already captures the field correctly.

---

## 5. Bundler Deprecation Fix

### Problem

The gem renderer uses `bundle install --deployment`, which is deprecated in
Bundler 2.1+.

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

Renderer-only change. Applies everywhere the old flag appears.

---

## 6. Driftify Coverage and Test Expectations

Extend driftify's `nonrpm` profile to generate test fixtures for new detection
capabilities.

### Fixtures

| Feature | Fixture |
|---------|---------|
| npm globals (unscoped) | Packages in `<prefix>/pm2/package.json`, `<prefix>/typescript/package.json` |
| npm globals (scoped) | Packages in `<prefix>/@angular/cli/package.json`, `<prefix>/@types/node/package.json` |
| npm globals (multi-prefix) | Packages in both `/usr/lib/node_modules/` and `/usr/local/lib/node_modules/` |
| C-extension (package subdir) | `.so` file at `site-packages/numpy/core/_multiarray.so` |
| C-extension (top-level) | `.so` file at `site-packages/ujson.cpython-311-x86_64-linux-gnu.so` |
| `--scan-home` paths | Language environments under user home dirs resolvable via `getent passwd` |
| `/var/www` deployments | Django venv and Node.js project under `/var/www/` |
| `system_site_packages` | Venv with `include-system-site-packages = true` in `pyvenv.cfg` |

### Minimum Test Expectations

**Collector tests:**
- npm globals from `npm list -g --json` command output
- Directory walk fallback when `npm` is absent
- Scoped package (`@scope/pkg`) discovery in directory walk
- RPM ownership filtering for global packages
- Multiple global prefixes produce separate environments
- C-extension detection for both package-subdir and top-level `.so` layouts
- C-extension false when no `.so` files present

**CLI tests:**
- `--scan-home` bare-flag produces error with help text
- `--scan-home all` discovers users with UID >= 1000
- `--scan-home nonexistent` warns and continues
- `--scan-home nginx` (system user) is included when explicitly named
- `--scan-path /nonexistent` warns and continues
- Broad `--scan-path /` produces warning
- `--inspect-only` stdout remains parseable JSON with new flags active

**Renderer tests:**
- npm globals rendered unpinned by default
- npm globals rendered pinned when `pinned: true` on packages
- `--system-site-packages` flag appears in venv creation when set
- Bundler new syntax in all gem rendering branches

**Refine/session tests:**
- `SetPackagePin` op round-trips through autosave/reload
- `SetBulkPackagePin` sets all packages in target environment
- Pin state survives session reload
- Exported Containerfile matches pinned/unpinned state

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
| LanguagePackage type | `crates/core/src/types/nonrpm.rs` |
| Snapshot meta | `crates/core/src/snapshot.rs` |
| Language package renderer | `crates/pipeline/src/render/language_packages.rs` |
| Method constants | `crates/core/src/util.rs` |
| Refine types (ItemId, RefinementOp) | `crates/refine/src/types.rs` |
| Refine session | `crates/refine/src/session.rs` |
| Web DTO types | `crates/web/src/web_types.rs` |
| Web adapter | `crates/web/src/adapter.rs` |
| Refine UI — language packages | `crates/web/ui/src/components/LanguagePackageList.tsx` |
| TypeScript types | `crates/web/ui/src/api/types.ts` |
| CLI scan command | `crates/cli/src/commands/scan.rs` |
| driftify nonrpm profile | `src/profiles/nonrpm.rs` (in driftify repo) |
