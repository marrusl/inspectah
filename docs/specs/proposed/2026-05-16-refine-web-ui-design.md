# Refine Web UI Design

**Phase:** 4 (Web UI for Refine)
**Date:** 2026-05-16
**Status:** APPROVED
**Depends on:** Phase 3 (Refine Service Layer) — COMPLETE
**Team input:** Fern (UX, 5 consults + 2 reviews), Ember (strategy, 3 reviews), Tang (engineering, 4 revisions), Kit (implementation, 4 reviews), Thorn (code quality, 3 reviews), Slate (security, 2 reviews)
**Approved:** 2026-05-16 — all four review lanes (Fern, Kit, Thorn, Slate) approved after 4 rounds

## Overview

The refine web UI is the interactive frontend for `inspectah refine`. It
loads an inspection snapshot and presents a browser-based interface where
the operator makes include/exclude decisions on packages and config files,
guided by server-computed attention routing. The Containerfile preview
updates live after each decision. When satisfied, the operator exports a
refined tarball for `inspectah build`.

```
inspectah refine <tarball>
    → starts localhost axum server
    → opens browser to embedded React app
    → operator triages items, previews Containerfile
    → exports refined tarball
    → inspectah build <tarball> → container image
```

## Scope — V1

**In scope:**

- Package include/exclude with attention-guided triage
- Config file include/exclude with attention-guided triage
- Live Containerfile preview (collapsible panel)
- Undo/redo with keyboard shortcuts
- Informational sections (services, containers, users, etc.) as read-only context
- Search/filter within sections and globally
- Tarball export with stale-generation guard
- Full keyboard navigation (vim-style + standard)
- WCAG 2.2 AA accessibility
- Dark theme (PatternFly 6 dark)
- Responsive breakpoints (1280px, 1024px)

**Not in v1:**

- Fleet aggregate view (Phase 3b provides the API; UI deferred)
- TUI (ratatui — same RefineSession API, different transport)
- Architect feature (multi-artifact decomposition)
- Quadlet/flatpak/service interactive operations
- Light theme toggle
- Collaborative multi-operator review
- Persistent sessions

## Technology Stack

| Layer | Choice | Rationale |
|-------|--------|-----------|
| Framework | React 19 + Vite | Component architecture scales with features. Kit has React experience from PKA dashboard. |
| Design system | PatternFly 6 (`@patternfly/react-core`) | Red Hat design language. Operators familiar with console UIs. Accessible components out of the box. |
| Embedding | `rust-embed` | Vite builds to `dist/`, baked into CLI binary at compile time. Zero runtime dependency on Node. |
| HTTP server | axum (existing `inspectah-web` crate) | Already wired with 9 endpoints, CORS, origin guard. |
| Syntax highlighting | Prism.js or highlight.js (Dockerfile grammar) | Lightweight, no build-step dependency. CodeBlock component wraps it. |

### Embedded UI Build Contract

#### Directory Structure

```
inspectah-web/
  build.rs                   ← cargo build script: runs npm ci + vite build
  ui/                        ← React project root (NEW)
    package.json
    package-lock.json         ← committed (deterministic installs)
    vite.config.ts
    tsconfig.json
    index.html
    src/                      ← committed source
      main.tsx
      App.tsx
      components/
      hooks/
      api/
    node_modules/             ← gitignored
    dist/                     ← Vite build output, gitignored
  src/
    lib.rs                    ← axum router
    handlers.rs
    assets.rs                 ← rust-embed reads from ui/dist/
    error.rs
  static/                     ← REMOVED (replaced by ui/dist/)
```

`rust-embed` reads from `ui/dist/`, not `static/`. The existing
`static/` directory and its placeholder `index.html` are deleted when
the React project is scaffolded. The `assets.rs` embed attribute
changes from `#[folder = "static/"]` to `#[folder = "ui/dist/"]`.

#### Build Pipeline

`build.rs` in the `inspectah-web` crate is the single authority for
building frontend assets. No Makefile, no "or" — `cargo build` drives
everything.

**build.rs sketch:**

```rust
// inspectah-web/build.rs
use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let ui_dir = Path::new("ui");

    // Tell cargo when to re-run this script
    println!("cargo:rerun-if-changed=ui/src");
    println!("cargo:rerun-if-changed=ui/package.json");
    println!("cargo:rerun-if-changed=ui/package-lock.json");
    println!("cargo:rerun-if-changed=ui/vite.config.ts");
    println!("cargo:rerun-if-changed=ui/tsconfig.json");
    println!("cargo:rerun-if-changed=ui/index.html");

    let dist_dir = ui_dir.join("dist");

    // Check if npm/node is available
    let has_npm = Command::new("npm").arg("--version").output().is_ok();

    if has_npm {
        // Install dependencies (ci for deterministic installs)
        let status = Command::new("npm")
            .arg("ci")
            .current_dir(ui_dir)
            .status()
            .expect("failed to run npm ci");
        assert!(status.success(), "npm ci failed");

        // Build production assets
        let status = Command::new("npm")
            .args(["run", "build"])
            .current_dir(ui_dir)
            .status()
            .expect("failed to run npm run build");
        assert!(status.success(), "npm run build failed");
    } else if dist_dir.exists() {
        // Node not installed but prior build exists — use stale assets
        println!(
            "cargo:warning=Node.js not found. Using existing ui/dist/ \
             from a prior build. Assets may be stale."
        );
    } else {
        // No Node, no prior build — hard fail
        panic!(
            "\n\n\
             ERROR: Node.js and npm are required to build inspectah-web.\n\
             \n\
             Install Node.js (>= 20 LTS): https://nodejs.org/\n\
             Then run: cargo build\n\
             \n\
             If you are only working on Rust code in other crates,\n\
             use: cargo build -p inspectah-cli\n\
             \n"
        );
    }
}
```

**Key behaviors:**

- `cargo:rerun-if-changed` targets `ui/src`, `ui/package.json`,
  `ui/package-lock.json`, `ui/vite.config.ts`, `ui/tsconfig.json`,
  and `ui/index.html`. Cargo skips the build script entirely when
  none of these change.
- `npm ci` (not `npm install`) for reproducible dependency resolution
  from the lockfile.
- Build output lands in `ui/dist/` where `rust-embed` picks it up.

#### Node-Absent Behavior

This is a developer-ergonomics concern. CI always has Node.

| Condition | Behavior |
|-----------|----------|
| Node installed | `npm ci && npm run build` runs, fresh assets in `ui/dist/` |
| Node absent, `ui/dist/` exists | Cargo warning printed, stale assets used, build succeeds |
| Node absent, no `ui/dist/` | `panic!` with install instructions and a hint to build other crates with `-p` |

The stale-asset path lets pure-Rust contributors (`inspectah-core`,
`inspectah-pipeline`, etc.) run `cargo build --workspace` after a prior
full build without installing Node. If they never built `inspectah-web`
before, the error message is explicit.

#### CI Integration

The GitHub Actions workflow must set up Node before the Rust build.
Add to the `tier1` job, before the existing `cargo fmt` step:

```yaml
# In .github/workflows/rust-ci.yml, tier1 job steps:

      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'
          cache-dependency-path: inspectah-web/ui/package-lock.json

      # No explicit npm ci / npm run build step needed —
      # build.rs handles it during cargo build.
      # Node setup + cache is all CI provides.
```

**Cache strategy:** `actions/setup-node@v4` with `cache: 'npm'` caches
`~/.npm` keyed on `inspectah-web/ui/package-lock.json`. `npm ci` inside
`build.rs` restores from cache, installs to `node_modules/` (not cached,
ephemeral per run). This is simpler than caching `node_modules/` directly
and avoids platform mismatch issues.

**Freshness guarantee:** `build.rs` always runs `npm ci && npm run build`
when Node is present and any tracked file changed. No hash check needed —
cargo's `rerun-if-changed` plus `npm ci` from lockfile is deterministic.
CI gets fresh assets on every build that touches UI source.

**Tier2 (Fedora container):** The `tier2` job runs in a `fedora:latest`
container for `ffi-rpm` testing. It does not need Node — it only tests
Rust crate behavior with librpm. `build.rs` will fail because no prior
`ui/dist/` exists in a fresh container. Fix: add `nodejs npm` to the
`dnf install` line in `tier2`, OR scope `tier2` to exclude
`inspectah-web` with `cargo test --workspace --features ffi-rpm
--exclude inspectah-web`. The latter is cleaner — tier2 tests librpm
integration, not the web UI.

#### Gitignore Rules

Add to the project `.gitignore`:

```gitignore
# UI build artifacts (inspectah-web)
inspectah-web/ui/node_modules/
inspectah-web/ui/dist/
```

| Path | Status |
|------|--------|
| `inspectah-web/ui/src/` | Committed |
| `inspectah-web/ui/package.json` | Committed |
| `inspectah-web/ui/package-lock.json` | Committed |
| `inspectah-web/ui/vite.config.ts` | Committed |
| `inspectah-web/ui/tsconfig.json` | Committed |
| `inspectah-web/ui/index.html` | Committed |
| `inspectah-web/ui/node_modules/` | Gitignored |
| `inspectah-web/ui/dist/` | Gitignored |
| `inspectah-web/build.rs` | Committed |
| `inspectah-web/static/` | Deleted (replaced by `ui/dist/`) |

#### Dev Workflow

**Iterating on the UI (hot-reload):**

```bash
cd inspectah-web/ui
npm install          # first time only
npm run dev          # Vite dev server on :5173, HMR enabled
```

In a second terminal:

```bash
cargo run -p inspectah-cli -- refine <tarball>
# axum server starts on :8642
```

Vite proxies API calls to the Rust backend. Edit React components, see
changes instantly via HMR. The Rust server does not need to restart for
UI changes.

**Testing with embedded assets (production-like):**

```bash
cd inspectah-web/ui && npm run build   # or let build.rs do it
cargo run -p inspectah-cli -- refine <tarball>
# browser loads embedded assets from ui/dist/
```

**vite.config.ts proxy snippet:**

```typescript
// inspectah-web/ui/vite.config.ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:8642',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',    // default, explicit for clarity
    emptyDirFirst: true,
  },
});
```

The proxy routes all `/api/*` requests to the Rust backend during
development. In production, the Rust binary serves both the API and
the embedded static assets from the same origin — no proxy needed.

## Architecture

### API Surface

**Existing endpoints (from Phase 3):**

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/health` | Health check |
| GET | `/api/view` | RefinedView: packages, configs, containerfile, stats, generation |
| POST | `/api/op` | Apply a RefinementOp |
| POST | `/api/undo` | Undo last operation |
| POST | `/api/redo` | Redo last undone operation |
| GET | `/api/ops` | Full operation history |
| GET | `/api/changes` | Change summary |
| POST | `/api/tarball` | Export tarball (requires generation match) |

**New endpoint for v1:**

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/snapshot/sections` | Informational sections (immutable for session lifetime) |

The `/api/snapshot/sections` endpoint returns all non-actionable section
data from the original snapshot: services, containers, users/groups,
network, storage, scheduled tasks, non-RPM software, kernel boot, SELinux.
This data never changes during a session — it is fetched once on page load.

### Data Flow

```
Page load:
  fetch /api/view           → packages, configs, containerfile, stats
  fetch /api/snapshot/sections → informational sections (one-time)

Toggle item:
  optimistic UI flip
  POST /api/op {op, target} → server applies, returns updated view
  re-render from response
  on error: revert toggle, show Alert toast

Undo/Redo:
  POST /api/undo or /api/redo → re-fetch /api/view → re-render

Export:
  confirmation dialog (change summary + generation)
  POST /api/tarball {generation} → download .tar.gz
  on stale generation: Alert, re-fetch view
```

### State Management

React state, no external state library. The API is the source of truth.

```typescript
interface AppState {
  view: RefinedView | null;          // from /api/view
  sections: SnapshotSections | null; // from /api/snapshot/sections (one-time)
  activeSection: string;             // sidebar selection
  previewOpen: boolean;              // Containerfile panel state
  loading: boolean;                  // initial load
  error: AppError | null;            // global error state
}
```

Optimistic updates are local — the toggle flips in React state immediately,
then the POST response replaces the entire view. No partial patching.

### Context Sections Contract

Context sections (the read-only sidebar items: Services, Containers, Users,
etc.) are served via `GET /api/snapshot/sections` as a normalized wire type.

#### Wire types

```rust
// inspectah-web/src/handlers.rs — presentation DTOs, NOT domain types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSection {
    pub id: String,           // e.g. "services", "containers"
    pub display_name: String, // e.g. "Services", "Containers"
    pub items: Vec<ContextItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: String,              // stable, unique within section
    pub title: String,           // display name
    pub subtitle: Option<String>,// secondary info
    pub detail: Option<String>,  // expandable content (rendered in detail pane)
    pub searchable_text: String, // concatenated searchable fields
}
```

#### Normalization layer

Normalization lives in `inspectah-web/src/handlers.rs` — a set of
`normalize_for_context()` functions that convert domain types to
`ContextSection`/`ContextItem`. These are presentation DTOs, not domain
model changes. The `inspectah-core` types remain untouched.

**Action fields are stripped during normalization.** Fields like `include`,
`tie`, `tie_winner`, `acknowledged`, and `fleet` are action/decision fields
that belong on the refine side. Context items are read-only — no action
fields pass through to `ContextItem`.

#### Section-to-ContextItem mapping

Each snapshot section maps to a `ContextSection` via `normalize_for_context()`.
The mapping from domain types to `ContextItem` fields:

| Section | Source type(s) | `id` | `title` | `subtitle` | `detail` | `searchable_text` |
|---------|---------------|------|---------|------------|----------|--------------------|
| `services` | `ServiceStateChange` | `unit` | `unit` | `"{current_state} → {action}"` | drop-in content if `SystemdDropIn` exists for same unit | `"{unit} {current_state} {default_state} {action}"` |
| `services` | `SystemdDropIn` (standalone, no matching state_change) | `path` | `unit` | `"drop-in"` | `content` | `"{unit} {path} {content}"` |
| `containers` | `QuadletUnit` | `name` | `name` | `image` | `content` (quadlet unit file) | `"{name} {image} {path}"` |
| `containers` | `ComposeFile` | `path` | basename of `path` | services list from `images` | `None` | `"{path} {service names}"` |
| `containers` | `RunningContainer` | `id` | `name` | `"{image} ({status})"` | env vars, mounts summary | `"{name} {image} {status}"` |
| `containers` | `FlatpakApp` | `app_id` | `app_id` | `"{origin}/{branch}"` | `None` | `"{app_id} {origin} {branch}"` |
| `users` | `serde_json::Value` (user) | `name` field | `name` field | `"uid:{uid}"` | sudoers rules, SSH key refs | JSON-extracted `name`, `uid` |
| `users` | `serde_json::Value` (group) | `name` field | `name` field | `"gid:{gid}"` | `None` | JSON-extracted `name`, `gid` |
| `network` | `NMConnection` | `name` | `name` | `"{conn_type} ({method})"` | `None` | `"{name} {conn_type} {method} {path}"` |
| `network` | `FirewallZone` | `name` | `name` | services/ports summary | `content` | `"{name} {services} {ports} {rich_rules}"` |
| `network` | `FirewallDirectRule` | `"{ipv}:{chain}:{priority}"` | `"{chain}"` | `"{ipv} {table}"` | `args` | `"{ipv} {table} {chain} {priority} {args}"` |
| `network` | `StaticRouteFile` | `path` | `name` | `path` | `None` | `"{path} {name}"` |
| `network` | `String` (ip_routes entry) | route string | route string | `"ip route"` | `None` | route string |
| `network` | `String` (ip_rules entry) | rule string | rule string | `"ip rule"` | `None` | rule string |
| `network` | `String` (hosts_additions entry) | hosts line | hosts line | `"hosts"` | `None` | hosts line |
| `network` | `ProxyEntry` | `"{source}:{line}"` | `source` | `line` | `None` | `"{source} {line}"` |
| `storage` | `FstabEntry` | `mount_point` | `mount_point` | `"{device} ({fstype})"` | `options` | `"{device} {mount_point} {fstype} {options}"` |
| `storage` | `MountPoint` | `target` | `target` | `"{source} ({fstype})"` | `options` | `"{target} {source} {fstype}"` |
| `storage` | `LvmVolume` | `"{vg_name}/{lv_name}"` | `lv_name` | `"VG: {vg_name}, size: {lv_size}"` | `None` | `"{lv_name} {vg_name} {lv_size}"` |
| `storage` | `VarDirectory` | `path` | `path` | `"~{size_estimate}"` | `recommendation` | `"{path} {size_estimate} {recommendation}"` |
| `storage` | `CredentialRef` | `credential_path` | `credential_path` | `"mount: {mount_point}"` | `source` | `"{credential_path} {mount_point} {source}"` |
| `kernel` | `SysctlOverride` | `key` | `key` | `"{runtime}" (default: "{default}")` | `source` | `"{key} {runtime} {default} {source}"` |
| `kernel` | `KernelModule` (non_default_modules) | `name` | `name` | `"size: {size}"` | `used_by` | `"{name} {size} {used_by}"` |
| `kernel` | `ConfigSnippet` (modules_load_d) | `path` | basename of `path` | `"modules-load.d"` | `content` | `"{path} {content}"` |
| `kernel` | `ConfigSnippet` (modprobe_d) | `path` | basename of `path` | `"modprobe.d"` | `content` | `"{path} {content}"` |
| `kernel` | `ConfigSnippet` (dracut_conf) | `path` | basename of `path` | `"dracut.conf.d"` | `content` | `"{path} {content}"` |
| `kernel` | `ConfigSnippet` (tuned_custom_profiles) | `path` | basename of `path` | `"tuned profile"` | `content` | `"{path} {content}"` |
| `kernel` | `AlternativeEntry` | `name` | `name` | `"{path} ({status})"` | `None` | `"{name} {path} {status}"` |
| `scheduled` | `CronJob` | `path` | basename of `path` | `source` | cron file content via `source` | `"{path} {source}"` |
| `scheduled` | `SystemdTimer` | `name` | `name` | `on_calendar` | `description`, `exec_start` | `"{name} {on_calendar} {exec_start} {description}"` |
| `scheduled` | `AtJob` | `file` | `file` | `"{user}: {command}"` | `working_dir` | `"{file} {command} {user}"` |
| `scheduled` | `GeneratedTimerUnit` | `name` | `name` | `cron_expr` | `source_path`, `command` | `"{name} {cron_expr} {source_path} {command}"` |
| `nonrpm` | `NonRpmItem` | `name` | `name` | `"{method} ({confidence})"` | `path`, pip packages if present | `"{name} {path} {method} {lang}"` |
| `nonrpm` | `ConfigFileEntry` (env_files) | `path` | basename of `path` | `"{kind}"` | `content` | `"{path} {content}"` |
| `selinux` | `SelinuxPortLabel` | `"{protocol}/{port}"` | `"{protocol}/{port}"` | `label_type` | `None` | `"{protocol} {port} {label_type}"` |
| `selinux` | `serde_json::Value` (boolean override) | JSON `name` field | JSON `name` field | current value | `None` | JSON-extracted fields |
| `selinux` | `String` (custom module) | module name | module name | `"custom module"` | `None` | module name |
| `selinux` | `String` (fcontext_rules entry) | rule string | rule string | `"fcontext"` | `None` | rule string |
| `selinux` | `CarryForwardFile` (audit_rules) | `path` | basename of `path` | `"audit rule"` | `content` | `"{path} {content}"` |
| `selinux` | `CarryForwardFile` (pam_configs) | `path` | basename of `path` | `"PAM config"` | `content` | `"{path} {content}"` |

#### Exhaustive field disposition

Every field of every snapshot section type is accounted for below. Fields
are classified as **Mapped** (becomes a ContextItem field), **Stripped**
(action/decision field removed during normalization), **Omitted** (not shown
in v1 with reason), or **Elsewhere** (served via a different mechanism).

##### `ServiceSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `state_changes` | `Vec<ServiceStateChange>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `drop_ins` | `Vec<SystemdDropIn>` | **Mapped:** standalone drop-ins become ContextItems; drop-ins matching a state_change unit are folded into that item's `detail` |
| `enabled_units` | `Vec<String>` | **Mapped:** each string becomes a ContextItem with `id`=unit name, `title`=unit name, `subtitle`=`"enabled"`, `detail`=`None` |
| `disabled_units` | `Vec<String>` | **Mapped:** each string becomes a ContextItem with `id`=unit name, `title`=unit name, `subtitle`=`"disabled"`, `detail`=`None` |

*ServiceStateChange fields:*

| Field | Disposition |
|-------|-------------|
| `unit` | **Mapped:** → `id`, `title` |
| `current_state` | **Mapped:** → `subtitle`, `searchable_text` |
| `default_state` | **Mapped:** → `searchable_text` |
| `action` | **Mapped:** → `subtitle`, `searchable_text` |
| `owning_package` | **Mapped:** → `searchable_text` (appended when present) |
| `include` | **Stripped** |
| `fleet` | **Stripped** |

*SystemdDropIn fields:*

| Field | Disposition |
|-------|-------------|
| `unit` | **Mapped:** → `title` |
| `path` | **Mapped:** → `id` (standalone) or lookup key |
| `content` | **Mapped:** → `detail`, `searchable_text` |
| `include` | **Stripped** |
| `tie` | **Stripped** |
| `tie_winner` | **Stripped** |
| `fleet` | **Stripped** |

##### `ContainerSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `quadlet_units` | `Vec<QuadletUnit>` | **Mapped:** see table above |
| `compose_files` | `Vec<ComposeFile>` | **Mapped:** see table above |
| `running_containers` | `Vec<RunningContainer>` | **Mapped:** see table above |
| `flatpak_apps` | `Vec<FlatpakApp>` | **Mapped:** see table above |

*QuadletUnit fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `searchable_text` |
| `name` | **Mapped:** → `id`, `title` |
| `content` | **Mapped:** → `detail` |
| `image` | **Mapped:** → `subtitle` |
| `ports` | **Mapped:** → `searchable_text` (appended when non-empty) |
| `volumes` | **Mapped:** → `searchable_text` (appended when non-empty) |
| `generated` | **Omitted:** internal flag, no user-facing meaning |
| `include` | **Stripped** |
| `tie` | **Stripped** |
| `tie_winner` | **Stripped** |
| `fleet` | **Stripped** |

*ComposeFile fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `id`, `title` (basename) |
| `images` | **Mapped:** → `subtitle` (service names joined) |
| `include` | **Stripped** |
| `tie` | **Stripped** |
| `tie_winner` | **Stripped** |
| `fleet` | **Stripped** |

*ComposeService fields (nested in ComposeFile.images):*

| Field | Disposition |
|-------|-------------|
| `service` | **Mapped:** → parent's `subtitle` (service names joined into comma-separated list) |
| `image` | **Mapped:** → parent's `searchable_text` (image refs appended for search) |

*RunningContainer fields:*

| Field | Disposition |
|-------|-------------|
| `id` | **Mapped:** → `id` |
| `name` | **Mapped:** → `title` |
| `image` | **Mapped:** → `subtitle`, `searchable_text` |
| `image_id` | **Omitted:** redundant with `image` for context display |
| `status` | **Mapped:** → `subtitle`, `searchable_text` |
| `restart_policy` | **Mapped:** → `searchable_text` (appended when non-empty) |
| `mounts` | **Mapped:** → `detail` (summarized list) |
| `networks` | **Omitted:** raw JSON blob, not useful in v1 context view |
| `ports` | **Omitted:** raw JSON blob, not useful in v1 context view |
| `env` | **Mapped:** → `detail` (summarized list) |
| `inspect_data` | **Omitted:** internal flag |
| `include` | **Stripped** |
| `acknowledged` | **Stripped** |
| `fleet` | **Stripped** |

*ContainerMount fields (nested in RunningContainer.mounts):*

| Field | Disposition |
|-------|-------------|
| `mount_type` | **Mapped:** → `detail` (part of mount summary, e.g. "bind /host:/ctr") |
| `source` | **Mapped:** → `detail` (part of mount summary) |
| `destination` | **Mapped:** → `detail` (part of mount summary) |
| `mode` | **Omitted:** low-level mount option, not actionable in v1 context |
| `rw` | **Omitted:** low-level mount flag, not actionable in v1 context |

*FlatpakApp fields:*

| Field | Disposition |
|-------|-------------|
| `app_id` | **Mapped:** → `id`, `title` |
| `origin` | **Mapped:** → `subtitle` |
| `branch` | **Mapped:** → `subtitle` |
| `remote` | **Mapped:** → `searchable_text` (appended when non-empty) |
| `remote_url` | **Mapped:** → `searchable_text` (appended when non-empty) |
| `include` | **Stripped** |

##### `NetworkSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `connections` | `Vec<NMConnection>` | **Mapped:** see table above |
| `firewall_zones` | `Vec<FirewallZone>` | **Mapped:** see table above |
| `firewall_direct_rules` | `Vec<FirewallDirectRule>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `static_routes` | `Vec<StaticRouteFile>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `ip_routes` | `Vec<String>` | **Mapped:** each string becomes a ContextItem (see table above) |
| `ip_rules` | `Vec<String>` | **Mapped:** each string becomes a ContextItem (see table above) |
| `resolv_provenance` | `String` | **Mapped:** single ContextItem with `id`=`"resolv_provenance"`, `title`=`"DNS resolver"`, `subtitle`=value, `detail`=`None` |
| `hosts_additions` | `Vec<String>` | **Mapped:** each string becomes a ContextItem (see table above) |
| `proxy` | `Vec<ProxyEntry>` | **Mapped:** each entry becomes a ContextItem (see table above) |

*NMConnection fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `searchable_text` |
| `name` | **Mapped:** → `id`, `title` |
| `method` | **Mapped:** → `subtitle`, `searchable_text` |
| `conn_type` | **Mapped:** → `subtitle` |
| `include` | **Stripped** |
| `acknowledged` | **Stripped** |
| `fleet` | **Stripped** |

*FirewallZone fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `searchable_text` |
| `name` | **Mapped:** → `id`, `title` |
| `content` | **Mapped:** → `detail` |
| `services` | **Mapped:** → `subtitle` |
| `ports` | **Mapped:** → `subtitle` |
| `rich_rules` | **Mapped:** → `searchable_text` |
| `include` | **Stripped** |
| `fleet` | **Stripped** |

*FirewallDirectRule fields:*

| Field | Disposition |
|-------|-------------|
| `ipv` | **Mapped:** → `id` (part), `subtitle` |
| `table` | **Mapped:** → `subtitle` |
| `chain` | **Mapped:** → `id` (part), `title` |
| `priority` | **Mapped:** → `id` (part), `searchable_text` |
| `args` | **Mapped:** → `detail`, `searchable_text` |
| `include` | **Stripped** |

*StaticRouteFile fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `id`, `subtitle` |
| `name` | **Mapped:** → `title` |

*ProxyEntry fields:*

| Field | Disposition |
|-------|-------------|
| `source` | **Mapped:** → `id` (part), `title` |
| `line` | **Mapped:** → `id` (part), `subtitle` |

##### `StorageSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `fstab_entries` | `Vec<FstabEntry>` | **Mapped:** see table above |
| `mount_points` | `Vec<MountPoint>` | **Mapped:** see table above |
| `lvm_info` | `Vec<LvmVolume>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `var_directories` | `Vec<VarDirectory>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `credential_refs` | `Vec<CredentialRef>` | **Mapped:** each entry becomes a ContextItem (see table above) |

*FstabEntry fields:*

| Field | Disposition |
|-------|-------------|
| `device` | **Mapped:** → `subtitle`, `searchable_text` |
| `mount_point` | **Mapped:** → `id`, `title` |
| `fstype` | **Mapped:** → `subtitle`, `searchable_text` |
| `options` | **Mapped:** → `detail`, `searchable_text` |
| `include` | **Stripped** |
| `acknowledged` | **Stripped** |
| `fleet` | **Stripped** |

*MountPoint fields:*

| Field | Disposition |
|-------|-------------|
| `target` | **Mapped:** → `id`, `title` |
| `source` | **Mapped:** → `subtitle`, `searchable_text` |
| `fstype` | **Mapped:** → `subtitle`, `searchable_text` |
| `options` | **Mapped:** → `detail`, `searchable_text` |

*LvmVolume fields:*

| Field | Disposition |
|-------|-------------|
| `lv_name` | **Mapped:** → `id` (part), `title` |
| `vg_name` | **Mapped:** → `id` (part), `subtitle` |
| `lv_size` | **Mapped:** → `subtitle`, `searchable_text` |

*VarDirectory fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `id`, `title` |
| `size_estimate` | **Mapped:** → `subtitle` |
| `recommendation` | **Mapped:** → `detail`, `searchable_text` |

*CredentialRef fields:*

| Field | Disposition |
|-------|-------------|
| `mount_point` | **Mapped:** → `subtitle` |
| `credential_path` | **Mapped:** → `id`, `title` |
| `source` | **Mapped:** → `detail`, `searchable_text` |

##### `KernelBootSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `sysctl_overrides` | `Vec<SysctlOverride>` | **Mapped:** see table above |
| `non_default_modules` | `Vec<KernelModule>` | **Mapped:** see table above |
| `modules_load_d` | `Vec<ConfigSnippet>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `modprobe_d` | `Vec<ConfigSnippet>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `dracut_conf` | `Vec<ConfigSnippet>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `tuned_custom_profiles` | `Vec<ConfigSnippet>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `alternatives` | `Vec<AlternativeEntry>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `cmdline` | `String` | **Mapped:** single ContextItem with `id`=`"cmdline"`, `title`=`"Kernel cmdline"`, `subtitle`=truncated value, `detail`=full value |
| `grub_defaults` | `String` | **Mapped:** single ContextItem with `id`=`"grub_defaults"`, `title`=`"GRUB defaults"`, `subtitle`=`None`, `detail`=full value |
| `tuned_active` | `String` | **Mapped:** single ContextItem with `id`=`"tuned_active"`, `title`=`"Active tuned profile"`, `subtitle`=value, `detail`=`None` |
| `locale` | `Option<String>` | **Mapped:** single ContextItem with `id`=`"locale"`, `title`=`"Locale"`, `subtitle`=value, `detail`=`None`. Omitted when `None`. |
| `timezone` | `Option<String>` | **Mapped:** single ContextItem with `id`=`"timezone"`, `title`=`"Timezone"`, `subtitle`=value, `detail`=`None`. Omitted when `None`. |
| `loaded_modules` | `Vec<KernelModule>` | **Omitted:** too large and noisy for context view; `non_default_modules` covers the actionable subset |

*SysctlOverride fields:*

| Field | Disposition |
|-------|-------------|
| `key` | **Mapped:** → `id`, `title` |
| `runtime` | **Mapped:** → `subtitle` |
| `default` | **Mapped:** → `subtitle` |
| `source` | **Mapped:** → `detail`, `searchable_text` |
| `include` | **Stripped** |

*KernelModule fields:*

| Field | Disposition |
|-------|-------------|
| `name` | **Mapped:** → `id`, `title` |
| `size` | **Mapped:** → `subtitle` |
| `used_by` | **Mapped:** → `detail` |
| `include` | **Stripped** |

*ConfigSnippet fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `id`, `title` (basename) |
| `content` | **Mapped:** → `detail`, `searchable_text` |

*AlternativeEntry fields:*

| Field | Disposition |
|-------|-------------|
| `name` | **Mapped:** → `id`, `title` |
| `path` | **Mapped:** → `subtitle` |
| `status` | **Mapped:** → `subtitle` |

##### `ScheduledTaskSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `cron_jobs` | `Vec<CronJob>` | **Mapped:** see table above |
| `systemd_timers` | `Vec<SystemdTimer>` | **Mapped:** see table above |
| `at_jobs` | `Vec<AtJob>` | **Mapped:** see table above |
| `generated_timer_units` | `Vec<GeneratedTimerUnit>` | **Mapped:** each entry becomes a ContextItem (see table above) |

*CronJob fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `id`, `title` (basename) |
| `source` | **Mapped:** → `subtitle`, `searchable_text` |
| `rpm_owned` | **Omitted:** informational flag, not user-facing in v1 |
| `include` | **Stripped** |
| `fleet` | **Stripped** |

*SystemdTimer fields:*

| Field | Disposition |
|-------|-------------|
| `name` | **Mapped:** → `id`, `title` |
| `on_calendar` | **Mapped:** → `subtitle` |
| `exec_start` | **Mapped:** → `detail`, `searchable_text` |
| `description` | **Mapped:** → `detail`, `searchable_text` |
| `source` | **Mapped:** → `searchable_text` |
| `path` | **Mapped:** → `searchable_text` |
| `timer_content` | **Omitted:** raw unit file, `on_calendar`+`exec_start`+`description` cover the useful parts |
| `service_content` | **Omitted:** raw unit file, covered by `exec_start` |
| `include` | **Stripped** |
| `fleet` | **Stripped** |

*AtJob fields:*

| Field | Disposition |
|-------|-------------|
| `file` | **Mapped:** → `id`, `title` |
| `command` | **Mapped:** → `subtitle`, `searchable_text` |
| `user` | **Mapped:** → `subtitle`, `searchable_text` |
| `working_dir` | **Mapped:** → `detail` |
| `include` | **Stripped** |
| `fleet` | **Stripped** |

*GeneratedTimerUnit fields:*

| Field | Disposition |
|-------|-------------|
| `name` | **Mapped:** → `id`, `title` |
| `cron_expr` | **Mapped:** → `subtitle`, `searchable_text` |
| `source_path` | **Mapped:** → `detail`, `searchable_text` |
| `timer_content` | **Omitted:** raw unit file content, `cron_expr`+`command` cover the useful parts |
| `service_content` | **Omitted:** raw unit file content, covered by `command` |
| `command` | **Mapped:** → `detail`, `searchable_text` |
| `include` | **Stripped** |
| `fleet` | **Stripped** |

##### `NonRpmSoftwareSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `items` | `Vec<NonRpmItem>` | **Mapped:** see table above |
| `env_files` | `Vec<ConfigFileEntry>` | **Mapped:** each entry becomes a ContextItem (see table above) |

*NonRpmItem fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `detail`, `searchable_text` |
| `name` | **Mapped:** → `id`, `title` |
| `method` | **Mapped:** → `subtitle` |
| `confidence` | **Mapped:** → `subtitle` |
| `lang` | **Mapped:** → `searchable_text` |
| `static` | **Omitted:** internal linkage flag, not useful in context view |
| `version` | **Mapped:** → `searchable_text` (appended when non-empty) |
| `shared_libs` | **Omitted:** too low-level for context display in v1 |
| `system_site_packages` | **Omitted:** Python-specific detail, not actionable in context |
| `packages` | **Mapped:** → `detail` (pip package name+version list when non-empty) |
| `has_c_extensions` | **Omitted:** Python-specific build detail, not actionable in context view |
| `git_remote` | **Omitted:** source provenance detail, not useful in v1 context display |
| `git_commit` | **Omitted:** source provenance detail, not useful in v1 context display |
| `git_branch` | **Omitted:** source provenance detail, not useful in v1 context display |
| `files` | **Omitted:** raw file listing (JSON blob), too verbose for context view |
| `content` | **Omitted:** raw file content, not useful in context display |
| `include` | **Stripped** |
| `acknowledged` | **Stripped** |
| `fleet` | **Stripped** |
| `review_status` | **Stripped** |
| `notes` | **Stripped** |

*PipPackage fields (nested in NonRpmItem.packages):*

| Field | Disposition |
|-------|-------------|
| `name` | **Mapped:** → `detail` (part of pip package list) |
| `version` | **Mapped:** → `detail` (part of pip package list) |

*ConfigFileEntry fields (when used as env_files):*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `id`, `title` (basename) |
| `kind` | **Mapped:** → `subtitle` |
| `category` | **Omitted:** redundant with section context (these are always `Environment`) |
| `content` | **Mapped:** → `detail`, `searchable_text` |
| `rpm_va_flags` | **Omitted:** RPM-internal detail |
| `package` | **Mapped:** → `searchable_text` (appended when present) |
| `diff_against_rpm` | **Omitted:** not shown in context view, belongs to config section's diff logic |
| `include` | **Stripped** |
| `tie` | **Stripped** |
| `tie_winner` | **Stripped** |
| `fleet` | **Stripped** |

##### `SelinuxSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `port_labels` | `Vec<SelinuxPortLabel>` | **Mapped:** see table above |
| `boolean_overrides` | `Vec<serde_json::Value>` | **Mapped:** see table above |
| `custom_modules` | `Vec<String>` | **Mapped:** see table above |
| `fcontext_rules` | `Vec<String>` | **Mapped:** each string becomes a ContextItem (see table above) |
| `audit_rules` | `Vec<CarryForwardFile>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `pam_configs` | `Vec<CarryForwardFile>` | **Mapped:** each entry becomes a ContextItem (see table above) |
| `mode` | `String` | **Mapped:** single ContextItem with `id`=`"selinux_mode"`, `title`=`"SELinux mode"`, `subtitle`=value (e.g. `"enforcing"`), `detail`=`None` |
| `fips_mode` | `bool` | **Mapped:** single ContextItem with `id`=`"fips_mode"`, `title`=`"FIPS mode"`, `subtitle`=`"enabled"` or `"disabled"`, `detail`=`None` |

*SelinuxPortLabel fields:*

| Field | Disposition |
|-------|-------------|
| `protocol` | **Mapped:** → `id` (part), `title` |
| `port` | **Mapped:** → `id` (part), `title` |
| `label_type` | **Mapped:** → `subtitle` |
| `include` | **Stripped** |
| `fleet` | **Stripped** |

*CarryForwardFile fields:*

| Field | Disposition |
|-------|-------------|
| `path` | **Mapped:** → `id`, `title` (basename) |
| `content` | **Mapped:** → `detail`, `searchable_text` |

##### `UserGroupSection`

| Field | Type | Disposition |
|-------|------|-------------|
| `users` | `Vec<serde_json::Value>` | **Mapped:** see table above |
| `groups` | `Vec<serde_json::Value>` | **Mapped:** see table above |
| `sudoers_rules` | `Vec<String>` | **Mapped:** folded into matching user ContextItems as `detail` content |
| `ssh_authorized_keys_refs` | `Vec<serde_json::Value>` | **Mapped:** folded into matching user ContextItems as `detail` content |
| `passwd_entries` | `Vec<String>` | **Omitted:** raw `/etc/passwd` lines — structured user data already covers this |
| `shadow_entries` | `Vec<String>` | **Omitted:** sensitive data, redaction boundary — never shown in UI |
| `group_entries` | `Vec<String>` | **Omitted:** raw `/etc/group` lines — structured group data already covers this |
| `gshadow_entries` | `Vec<String>` | **Omitted:** sensitive data, redaction boundary — never shown in UI |
| `subuid_entries` | `Vec<String>` | **Omitted:** rootless container plumbing, not actionable in v1 context |
| `subgid_entries` | `Vec<String>` | **Omitted:** rootless container plumbing, not actionable in v1 context |

##### `ConfigSection`

Not served via `/api/snapshot/sections`. Config files are actionable items
in the refine triage flow — they appear in the main content area as
include/exclude decisions, not as read-only context. The `ConfigFileEntry`
type is handled entirely by the mutation side (`/api/op`), not the context
side.

##### `RpmSection`

Not served via `/api/snapshot/sections`. RPM packages are the primary
actionable items in the refine triage flow. They appear in the main content
area as include/exclude decisions. The following fields are handled entirely
by the mutation side:

- `packages_added`, `base_image_only` — actionable package lists
- `rpm_va` — verification results, part of config flow
- `repo_files`, `gpg_keys` — actionable repo config items
- `version_changes` — informational, but surfaced in Containerfile preview logic
- `module_streams`, `version_locks` — actionable module/lock decisions
- `leaf_packages`, `auto_packages`, `leaf_dep_tree` — dependency analysis used by attention routing
- `dnf_history_removed`, `module_stream_conflicts`, `baseline_module_streams` — internal pipeline data
- `versionlock_command_output` — raw command output, not user-facing
- `multiarch_packages`, `duplicate_packages`, `repo_providing_packages` — diagnostic data
- `ostree_overrides`, `ostree_removals` — ostree-specific, handled by mutation side
- `base_image`, `baseline_package_names`, `no_baseline` — pipeline metadata
- `file_ownership` — internal data for RPM file attribution

##### Snapshot top-level fields

| Field | Type | Disposition |
|-------|------|-------------|
| `schema_version` | `u32` | **Elsewhere:** available in `/api/health` response |
| `meta` | `HashMap<String, Value>` | **Elsewhere:** `meta["hostname"]` served via host info (see below) |
| `os_release` | `Option<OsRelease>` | **Elsewhere:** served via host info (see below) |
| `system_type` | `SystemType` | **Elsewhere:** served via host info (see below) |
| `rpm` | `Option<RpmSection>` | **Elsewhere:** actionable items in main triage flow |
| `config` | `Option<ConfigSection>` | **Elsewhere:** actionable items in main triage flow |
| `services` | `Option<ServiceSection>` | **Mapped:** → `services` ContextSection |
| `network` | `Option<NetworkSection>` | **Mapped:** → `network` ContextSection |
| `storage` | `Option<StorageSection>` | **Mapped:** → `storage` ContextSection |
| `scheduled_tasks` | `Option<ScheduledTaskSection>` | **Mapped:** → `scheduled` ContextSection |
| `containers` | `Option<ContainerSection>` | **Mapped:** → `containers` ContextSection |
| `non_rpm_software` | `Option<NonRpmSoftwareSection>` | **Mapped:** → `nonrpm` ContextSection |
| `kernel_boot` | `Option<KernelBootSection>` | **Mapped:** → `kernel` ContextSection |
| `selinux` | `Option<SelinuxSection>` | **Mapped:** → `selinux` ContextSection |
| `users_groups` | `Option<UserGroupSection>` | **Mapped:** → `users` ContextSection |
| `preflight` | `PreflightResult` | **Omitted:** preflight data is pipeline-internal; refine operates post-preflight |
| `warnings` | `Vec<Warning>` | **Omitted:** inspection warnings are pipeline-time artifacts, not refine-time context |
| `redactions` | `Vec<RedactionFinding>` | **Omitted:** redaction metadata is security-internal; redacted content is already sanitized before the snapshot reaches refine |
| `redaction_hints` | `Vec<RedactionHint>` | **Omitted:** consumed by redaction engine before refine, not user-facing |
| `redaction_state` | `Option<RedactionState>` | **Omitted:** internal trust state for re-rendering |
| `completeness` | `Completeness` | **Elsewhere:** surfaced in `/api/health` response as degradation indicator |

#### Host/System Info

The sidebar `HostInfo` component displays system identity data that does
**not** come from `/api/snapshot/sections`. These fields are top-level
snapshot properties, not section items, and are served via a dedicated
response shape.

**Source fields:**

| Field | Source | Display |
|-------|--------|---------|
| Hostname | `meta["hostname"]` | Primary heading in HostInfo |
| OS name | `os_release.pretty_name` (fallback: `os_release.name`) | Subtitle line 1 |
| OS version | `os_release.version_id` | Subtitle line 1 (appended) |
| OS ID | `os_release.id` | Used for RHEL/Fedora/CentOS icon selection |
| System type | `system_type` | Badge: `"package-mode"`, `"bootc"`, `"rpm-ostree"` |

**Unused OsRelease fields in v1:**

| Field | Reason |
|-------|--------|
| `version` | Redundant with `version_id` + `pretty_name` |
| `id_like` | Internal metadata, not user-facing |
| `variant_id` | Relevant for ostree desktops, not refine context |

**Serving mechanism:** The `/api/health` endpoint is extended to include a
`host` key alongside the existing `status` field:

```rust
// GET /api/health response (extended)
{
    "status": "ok",
    "host": {
        "hostname": "prod-web-01.example.com",  // from meta["hostname"]
        "os_name": "Red Hat Enterprise Linux",  // os_release.pretty_name
        "os_version": "9.4",                    // os_release.version_id
        "os_id": "rhel",                        // os_release.id
        "system_type": "package-mode",          // system_type enum
        "schema_version": 14                    // snapshot schema_version
    },
    "completeness": "complete"                  // or "partial"/"incomplete"
}
```

The frontend fetches `/api/health` on load to populate the HostInfo sidebar
component and to detect degraded inspection completeness.

**Notes on `users` and `selinux` sections:** These use `serde_json::Value`
and `Vec<String>` respectively for some sub-items. The normalizer extracts
fields by key (`"name"`, `"uid"`, `"gid"`) with fallback to
`Value::to_string()` for `searchable_text`. Missing keys produce empty
strings, not errors.

#### Empty section handling

Sections with zero items after normalization are **omitted** from the
response. The client never sees an empty `items: []` — if a section isn't
in the response, it doesn't exist in the sidebar.

#### Search contract

`searchable_text` is the indexed field for global search. The search
endpoint returns:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub section_id: String,   // which ContextSection
    pub item_id: String,      // which ContextItem within it
    pub match_context: String, // snippet showing the match
}
```

Global search hits `searchable_text` across all sections and returns
`Vec<SearchResult>`. The UI highlights matching items in whichever section
they belong to.

### Mutation and Review-State Contract

#### Client mutation serialization

One mutation in flight at a time. The UI maintains a mutation queue:

```typescript
// Pseudocode — not a literal implementation spec
const mutationQueue: MutationRequest[] = [];
let inFlight: boolean = false;

function enqueueMutation(req: MutationRequest) {
  mutationQueue.push(req);
  drainQueue();
}

function drainQueue() {
  if (inFlight || mutationQueue.length === 0) return;
  inFlight = true;
  const next = mutationQueue.shift()!;
  applyOptimisticUpdate(next);
  postMutation(next)
    .then(response => { inFlight = false; applyServerState(response); drainQueue(); })
    .catch(err => { inFlight = false; revertAllPending(); mutationQueue.length = 0; });
}
```

When a toggle fires while another is in-flight, it queues. On error, the
queue drains and **all pending optimistic flips revert**.

#### Generation-based stale-response policy

Every API response includes a `generation: u64` field. The client tracks
`lastAppliedGeneration`. Any response where `generation < lastAppliedGeneration`
is silently discarded. This handles rapid-toggle race conditions where
responses arrive out of order.

```typescript
let lastAppliedGeneration = 0;

function applyServerState(response: ApiResponse) {
  if (response.generation < lastAppliedGeneration) return; // stale, discard
  lastAppliedGeneration = response.generation;
  setState(response.view);
}
```

#### Viewed tracking on RefineSession

The server tracks which items an operator has triaged. This state is
**non-serialized** — it lives on `RefineSession` in memory and is excluded
from tarball export.

```rust
// Added to RefineSession (non-serialized, excluded from tarball)
viewed: HashSet<String>,  // section:item_id keys for triaged items

impl RefineSession {
    pub fn mark_viewed(&mut self, id: &str) { self.viewed.insert(id.to_string()); }
    pub fn is_viewed(&self, id: &str) -> bool { self.viewed.contains(id) }
    pub fn viewed_ids(&self) -> &HashSet<String> { &self.viewed }
}
```

#### Viewed trigger events

An item transitions to "triaged" on one of two deliberate operator
actions. Navigation alone (j/k, arrow keys) never marks an item triaged.

| Operator action | Marks triaged? | Rationale |
|-----------------|---------------|-----------|
| **Toggle** include/exclude (Space/x) | Always | The operator made a disposition decision. |
| **Expand** detail card (Enter) on a non-toggled item | Yes | The operator inspected the item's details and chose to leave it in its default state. This is a deliberate "I dealt with this." |
| **Expand** detail card on an already-toggled item | No-op | Already triaged via toggle. |
| **Navigate** past item (j/k, ↑/↓) | Never | Scrolling past is orientation, not disposition. |
| **Search result** highlighting an item | Never | Finding is not triaging. |

The asymmetry is intentional: toggling is always a decision, but for
items left in their default state, the operator must actively inspect
(expand) to signal "I looked at this and it's fine." This prevents the
counter from decrementing on passive scroll-through.

#### Viewed key space

Viewed identifiers use the format `section:item_id` to guarantee
global uniqueness across sections:

| Section | Key format | Example |
|---------|-----------|---------|
| Packages | `packages:{name}.{arch}` | `packages:httpd.x86_64` |
| Config Files | `configs:{path}` | `configs:/etc/httpd/conf/httpd.conf` |

The `section:` prefix is required because `ContextItem.id` values
are only unique within their section. The web layer constructs the
prefixed key before calling `mark_viewed()`.

#### Viewed tracking endpoints

| Method | Path | Body | Response |
|--------|------|------|----------|
| `POST` | `/api/viewed` | `{"id": "packages:httpd.x86_64"}` | `204 No Content` |
| `GET` | `/api/viewed` | — | `{"ids": ["packages:httpd.x86_64", ...]}` |

#### NeedsReview count semantics

The server returns `needs_review_count` in the `RefinedView` response —
this is the **total** count of items with `NeedsReview` attention tags. It
never shrinks (items don't lose their attention tags by being triaged).

The client computes the **remaining** count:

```typescript
const remaining = needsReviewCount
  - viewedIds.filter(id => isNeedsReview(id)).length;
```

The stats bar shows progress as **"N of M remaining"** where M is the
total NeedsReview count (from server) and N is the client-computed
remaining count. Both numbers are visible — the denominator anchors
scope ("how big is this review?"), the numerator tracks progress.

When remaining reaches 0, the counter swaps to **"All triaged"** with
a checkmark. The word "triaged" (not "reviewed") honestly describes
session-local disposition tracking — the operator made a call on every
flagged item, but the tool does not claim to verify the depth of that
review. This framing scales to fleet operations where operators
pattern-match across hosts rather than deep-review each one.

#### Undo/redo request contract

Both `POST /api/undo` and `POST /api/redo` require a `{}` JSON body
(matching the current handler implementation — the handlers deserialize
an empty struct). Response includes the full `RefinedView`.

For focus restoration after undo/redo, the client tracks the last-applied
operation target **locally** before sending, then uses that to restore
keyboard focus after the response arrives. The server does not participate
in focus management.

#### Cleanup: strip dead `acknowledged` field

The `acknowledged: bool` field on `PackageEntry` in
`inspectah-core/src/types/rpm.rs` (line 39) is dead code — no logic reads
it, no UI exposes it, and it was superseded by the attention-tag system.
**Remove it in the implementation phase.** The same dead field exists on
`RunningContainer` (containers.rs, line 92), `NMConnection` (network.rs),
and `FstabEntry` (storage.rs). All four should be removed together.

## Layout

### Three-Zone Structure

```
┌──────────────────────────────────────────────────────────────┐
│  Stats Bar (PageSection, full-width, global state)           │
│  Pkgs: 142/8   Cfgs: 23/2   8 of 12 remaining   ↶ ↷   Export  │
└──────────────────────────────────────────────────────────────┘
┌────────────┬─────────────────────────────┬───────────────────┐
│ <nav>      │ <main>                      │ <aside>           │
│ Sidebar    │ Main Content                │ Containerfile     │
│            │                             │ Preview           │
│ DECISIONS  │ SearchFilter                │                   │
│ ● Packages │ [NeedsReview] expanded      │ ~280px wide       │
│ ● Configs  │ [Informational] collapsed   │ Collapsible to    │
│            │ [Routine] collapsed         │ 28px vertical tab │
│ CONTEXT    │                             │                   │
│ ○ Services │ EmptyState when filtered    │ Syntax-highlighted│
│ ○ Contrs.  │ to zero results             │ Live updates      │
│ ○ Users    │                             │                   │
│ ○ NonRPM   │ Skeleton on first load      │ Ctrl+E toggle     │
│ ○ System   │                             │                   │
│ ○ Kernel   │                             │                   │
│ ○ Storage  │                             │                   │
│ ○ Network  │                             │                   │
│ ○ SELinux  │                             │                   │
│ ○ Sched.   │                             │                   │
│            │                             │                   │
│ host info  │                             │ 15 lines          │
└────────────┴─────────────────────────────┴───────────────────┘
      Landmark regions for skip-navigation
```

### Stats Bar

Top-level `PageSection` spanning all three zones. Contains:

- **StatusCounts:** Package counts (included/excluded), config counts,
  triage progress ("N of M remaining")
- **UndoRedo:** Undo and redo buttons with disabled state driven by
  `stats.can_undo` / `stats.can_redo`
- **ExportButton:** Visually separated from counters (right-aligned,
  primary action styling). This is a terminal action, not a status
  indicator — it belongs in the bar for discoverability but must not
  blend with the counters.

### Sidebar

PatternFly `Nav` with `NavGroup` sections:

**Decisions group (expanded by default):**
- Packages — with `Badge` showing NeedsReview count (red)
- Config Files — with `Badge` showing NeedsReview count (amber if >0)

**Context group (collapsed by default):**
- Services
- Containers
- Users & Groups
- Non-RPM Software
- System Info
- Kernel Boot
- Storage
- Network
- SELinux
- Scheduled Tasks

Ordered by signal value: sections that directly inform include/exclude
decisions (services, containers, non-RPM) appear first.

**Global search:** `SearchInput` below the nav groups. Searches across
all sections by item name/path. Results show which section contains each
match.

**Host info:** Bottom of sidebar. Hostname, OS release, architecture.
Always visible for orientation.

### Main Content

Renders the active section. Two component variants:

**ActionableSection** (Packages, Configs):
- `SearchFilter` at top — inline filter triggered by `/`, filters by name,
  path, attention reason. `Escape` clears.
- Three `AttentionGroup` sub-sections:
  - **NeedsReview:** Expanded by default. Decision cards.
  - **Informational:** Collapsed by default. Compact rows with toggles.
  - **Routine:** Collapsed by default. Compact rows with toggles.

**InformationalSection** (Context sections):
- Flat list, muted left-border (gray vs. the colored borders on actionable
  items). No toggle switches. No attention sub-grouping.
- PatternFly `DataList` without selection.

### Containerfile Preview Panel

Persistent right panel implemented as a `PageSection` with controlled
width, NOT a `Drawer` (which is for contextual item-detail, not
persistent global context).

**Open state (~280px):**
- Header: "Containerfile" label + collapse chevron (`⟩`)
- Body: Syntax-highlighted Containerfile content (read-only `CodeBlock`
  with Dockerfile grammar)
- Footer: Line count, "updates live" indicator

**Collapsed state (28px):**
- Vertical tab with rotated "Containerfile" label
- Click tab or `Ctrl+E` to expand

**Behavior:**
- Updates live after each toggle (re-rendered from `/api/view` response)
- Smooth CSS transition on collapse/expand (`transition: width 200ms ease`)
- State persists in `localStorage`
- Keyboard: `Ctrl+E` toggles from anywhere

**Responsive:** Auto-collapses below 1280px viewport width.

## Component Tree

```
<App>
  <ErrorBoundary>
    <StatsBar>                            ← PageSection, full-width
      <StatusCounts />
      <UndoRedo />
      <ExportButton />
    </StatsBar>
    <PageLayout>
      <Sidebar as="nav">                  ← landmark region
        <NavGroup "Decisions">
          <NavItem "Packages" badge={needsReviewCount} />
          <NavItem "Configs" badge={needsReviewCount} />
        </NavGroup>
        <NavGroup "Context" collapsed>
          <NavItem "Services" />
          <NavItem "Containers" />
          ...
        </NavGroup>
        <SearchGlobal />
        <HostInfo />
      </Sidebar>
      <ErrorBoundary>
        <MainContent as="main">           ← landmark region
          <ActionableSection>
            <SearchFilter />
            <AttentionGroup level="NeedsReview" expanded>
              <DecisionCard />            ← role="row" w/ nested Switch in gridcell
            </AttentionGroup>
            <AttentionGroup level="Informational" collapsed>
              <CompactRow />
            </AttentionGroup>
            <AttentionGroup level="Routine" collapsed>
              <CompactRow />
            </AttentionGroup>
          </ActionableSection>
          — or —
          <InformationalSection>
            <DataListItem />              ← flat, muted, no toggles
          </InformationalSection>
        </MainContent>
      </ErrorBoundary>
      <ErrorBoundary>
        <ContainerfilePanel as="aside">   ← landmark region, PageSection
          <PanelHeader collapse={toggle} />
          <CodeBlock language="dockerfile" />
          <PanelFooter lineCount={n} />
        </ContainerfilePanel>
      </ErrorBoundary>
    </PageLayout>
  </ErrorBoundary>
</App>
```

Error boundaries on `MainContent` and `ContainerfilePanel` are
independent — a Containerfile render failure does not take down the
decision interface.

## Interaction Model

### Decision Cards (NeedsReview Items)

Full card with:
- Package/config name and version/path
- Attention reason as PatternFly `Label` (e.g., "Not in baseline", "Modified from RPM default", "Sensitive path")
- `Switch` toggle for include/exclude
- Expandable detail via chevron or `Enter`:
  - Packages: source repo, version, architecture, state
  - Configs: diff against RPM default (if available), file content preview, category, owning RPM

Left border color-coded by attention level:
- Red (`--pf-t--global--color--status--danger`) for NeedsReview
- Amber (`--pf-t--global--color--status--warning`) for Informational
- Green (`--pf-t--global--color--status--success`) for Routine

### Compact Rows (Informational/Routine Items)

Minimal: name + `Switch` in a single row, no card chrome.
Under a collapsible `ExpandableSection` with count badge.
Routine header: "130 packages — all included by default."

### Toggle Behavior

1. Operator clicks `Switch` or presses `Space`/`x`
2. Optimistic UI: toggle flips immediately in React state
3. `POST /api/op` fires with `{op: "ExcludePackage", target: {name, arch}}`
   or the corresponding Include/Config variant
4. On success: replace view state from response, Containerfile preview
   updates, stats bar counts update
5. On error: revert toggle to previous state, show transient `Alert` toast
   with error message (3 second auto-dismiss)

### Undo/Redo

- `Ctrl+Z` globally, or click undo button in stats bar
- `Ctrl+Shift+Z` globally, or click redo button
- Fires `POST /api/undo` or `/api/redo`, re-fetches view
- Buttons show disabled state when `stats.can_undo` / `stats.can_redo`
  is false
- **Focus management:** After undo/redo, focus moves to the affected
  item. If the item is no longer visible (filtered out or section
  collapsed), focus moves to the parent section header.

### Search/Filter

**Section-level (`/`):**
- Opens inline `SearchInput` above the active section's item list
- Filters by name, path, attention reason — real-time as the operator types
- `Escape` clears filter and restores full list
- `ArrowDown` from filter field moves focus to first matching item
- Shows `EmptyState` when filter produces zero matches

**Global (sidebar):**
- Always-visible `SearchInput` in sidebar
- Searches across all sections by item name/path
- Results indicate which section contains each match
- Selecting a result navigates to that section and highlights the item

### Export Workflow

1. Operator clicks "Export Tarball" in stats bar (or `Ctrl+Shift+E`)
2. Confirmation dialog (`Modal`):
   - Summary: "X packages excluded, Y configs excluded"
   - Shows current generation number
   - "Export" primary button, "Cancel" secondary
3. `POST /api/tarball` with `{generation: currentGeneration}`
4. Server validates generation matches session state:
   - Match: returns `.tar.gz` binary → browser downloads
   - Mismatch: returns 409 → `Alert` explaining stale state, auto
     re-fetches `/api/view`
5. Success: `Alert` toast "Exported inspectah-refine-output.tar.gz"

## Interaction / Accessibility Contract

This section is the single source of truth for keyboard behavior, ARIA
semantics, focus management, responsive layout, and search interaction.
Kit implements from this contract. If it contradicts anything above
(Component Tree, PatternFly Component Map, Layout), this section wins.

---

### 1. Sidebar: Keyboard and ARIA Model

**Pattern chosen: PatternFly Nav (links with `aria-current`).**

The sidebar is a flat navigation menu, not a hierarchical tree. It has
two visual groups (Decisions, Context) but no nested expand/collapse
semantics -- groups are always expanded and serve only as visual
dividers. The Go report uses `<nav>` with `<a>` links and
`aria-current="page"`, and that pattern is correct here.

A `role="tree"` would be appropriate if sidebar items had
parent/child expand-collapse relationships (e.g., Packages > RPMs >
kernel-related). They do not. Each NavItem is a leaf that activates a
section. Tree semantics would force screen readers to announce nesting
depth for every item, adding noise with no information gain.

**Implementation:**

```html
<nav aria-label="Section navigation">
  <div role="group" aria-labelledby="decisions-heading">
    <span id="decisions-heading" class="pf-v6-c-nav__section-title">
      Decisions
    </span>
    <ul role="list">
      <li><a href="#packages" aria-current="page">Packages <Badge /></a></li>
      <li><a href="#configs">Config Files <Badge /></a></li>
    </ul>
  </div>
  <div role="group" aria-labelledby="context-heading">
    <span id="context-heading" class="pf-v6-c-nav__section-title">
      Context
    </span>
    <ul role="list">
      <li><a href="#services">Services</a></li>
      <!-- ... -->
    </ul>
  </div>
</nav>
```

- The active section's link carries `aria-current="page"`.
- Links are standard `<a>` elements -- Tab moves between them
  naturally. No roving tabindex needed.
- `NavGroup` section titles (`Decisions`, `Context`) are
  `role="group"` containers with `aria-labelledby`, not expandable
  disclosure widgets.

**Keyboard:**

| Key | Action |
|-----|--------|
| `1`-`9` | Jump to sidebar section by position (see shortcut scoping, section 3) |
| `Tab` / `Shift+Tab` | Move between nav links (standard browser behavior) |
| `Enter` | Activate link -- load section, move focus to main content (see focus state machine, section 6) |

`ArrowUp`/`ArrowDown` within the sidebar are NOT implemented.
Rationale: PatternFly Nav uses standard link navigation (Tab), not
arrow-key roving. Adding arrow keys on top of Tab creates two
competing navigation models in the same widget. The Go report uses
arrows in the sidebar because it built a custom nav; the React
version uses PatternFly Nav components and follows their interaction
contract.

---

### 2. Item List: Keyboard and ARIA Model

**Pattern chosen: `role="grid"` with `role="row"` items and
`role="gridcell"` containers for interactive content.**

**Why the previous draft's `role="list"` + `role="listitem"` was
wrong:**

`role="list"` is a structural role, not a composite widget. ARIA does
not define managed focus (roving tabindex) for lists. Screen readers
treat `role="list"` as static content with no keyboard navigation
contract. Applying roving tabindex to a list is an ARIA violation --
assistive technology will not announce focus changes correctly.

**Why not `role="listbox"` + `role="option"`:**

`role="option"` forbids interactive descendants. A toggle switch
inside an option is invalid ARIA. Our items need three behaviors:
navigate, toggle, and expand.

**Why `role="grid"`:**

- Grid is a composite widget -- it supports managed focus via roving
  tabindex on rows.
- Grid rows can contain interactive gridcells -- the toggle switch
  lives in a gridcell and is reachable via Tab after focusing the row.
- Grid supports `aria-expanded` on rows for expand/collapse detail.
- PatternFly DataList internally maps to this pattern. The PF React
  DataList component renders `role="list"` by default, but PF
  documentation acknowledges grid semantics for interactive lists.
  We use explicit grid roles rather than relying on PF's default to
  avoid the invalid list+roving-tabindex combination.

**Implementation:**

```html
<div role="grid" aria-label="Packages needing review"
     aria-rowcount="47">
  <!-- One row per item -->
  <div role="row" tabindex="0" aria-rowindex="1"
       aria-expanded="false"
       data-key="httpd">
    <div role="gridcell">
      <span>httpd</span>
      <Label>Not in baseline</Label>
    </div>
    <div role="gridcell">
      <Switch role="switch" aria-checked="true"
              aria-label="httpd: included"
              tabindex="-1" />
    </div>
  </div>
  <!-- ... -->
</div>
```

**Roving tabindex rules:**

- Exactly one row in each grid has `tabindex="0"` (the focused row).
  All other rows have `tabindex="-1"`.
- When focus enters the grid (Tab from outside), the row with
  `tabindex="0"` receives focus. This is the last-focused row, or
  the first row on initial entry.
- `j`/`ArrowDown` moves `tabindex="0"` to the next row.
  `k`/`ArrowUp` moves it to the previous row. Focus wraps: down from
  last row goes to first, up from first goes to last.
- `Tab` from a focused row moves focus INTO the row's interactive
  elements (Switch, expand chevron, detail links). `Shift+Tab`
  returns to the row.
- `Escape` from inside a row's interactive elements returns focus to
  the row itself.

**Three behaviors on a focused row:**

| Key | Action | ARIA effect |
|-----|--------|-------------|
| `Space` or `x` | Toggle include/exclude | Activates the Switch in the row's gridcell. `aria-checked` flips. Live region announces change. |
| `Enter` | Expand/collapse detail | `aria-expanded` flips on the row. Detail panel renders below the row. |
| `Tab` | Enter row internals | Focus moves to first interactive element inside the row (the Switch). |

**Focus after toggle:** Focus stays on the current row.
`tabindex="0"` does not move. The operator decides pace.

**Nested Switch interaction:**

When the Switch inside a gridcell has focus (reached via Tab):
- `Space` or `Enter` toggles it (standard switch behavior).
- `ArrowUp`/`ArrowDown`/`j`/`k` are inert (do not navigate rows
  while focus is inside a gridcell).
- `Escape` or `Shift+Tab` returns focus to the parent row.

This means `Space` toggles the item regardless of whether focus is on
the row or on the Switch -- the row-level `Space` handler delegates
to the Switch.

**Attention group collapse/expand:**

Each `AttentionGroup` (`ExpandableSection`) has a header button with
`aria-expanded`. When collapsed, its grid is not rendered (removed
from DOM, not `display:none`). When expanded, the grid appears and
the first row receives `tabindex="0"`.

Tab order through a section:
`SearchFilter` -> `NeedsReview header` -> `NeedsReview grid` ->
`Informational header` -> (if expanded) `Informational grid` ->
`Routine header` -> (if expanded) `Routine grid`.

---

### 3. Single-Key Shortcut Scoping

Single-key shortcuts (`/`, `?`, `1`-`9`, `j`, `k`, `x`) are
application-level shortcuts (not standard browser keys). They must be
suppressed when they would conflict with text input or modal
interaction.

**Suppression rule (implement in `useKeyboardShortcuts`):**

Single-key shortcuts are INACTIVE when ANY of the following is true:

1. **Focus is inside a text input.** Test:
   `document.activeElement` matches `input[type="text"]`,
   `input[type="search"]`, `textarea`, or any element with
   `contenteditable`. This covers `SearchInput` (both section-level
   and global).

2. **A modal is open.** Test: any element with `role="dialog"` is
   present in the DOM. This covers the export confirmation modal, the
   shortcut overlay (`?`), and the file viewer modal.

3. **A modifier key is held.** `Ctrl`, `Alt`/`Option`, `Meta`/`Cmd`,
   or `Shift` is pressed alongside the key. Exception: `Shift` alone
   does not suppress `?` (which requires Shift on US keyboards).

**Modifier shortcuts** (`Ctrl+Z`, `Ctrl+Shift+Z`, `Ctrl+E`,
`Ctrl+Shift+E`) are ALWAYS active except inside a modal with
`role="dialog"`, where the modal's own key handlers take precedence.

**Implementation pattern:**

```typescript
function isShortcutSuppressed(event: KeyboardEvent): boolean {
  // Modifier shortcuts are never suppressed here
  if (event.ctrlKey || event.metaKey || event.altKey) return false;

  // Shift exception: allow ? (Shift+/)
  if (event.shiftKey && event.key !== '?') return false;

  const target = event.target as HTMLElement;
  const tag = target.tagName;

  // Inside text input
  if (tag === 'INPUT' || tag === 'TEXTAREA') return true;
  if (target.isContentEditable) return true;

  // Inside modal
  if (document.querySelector('[role="dialog"]')) return true;

  return false;
}
```

**`/` special case:** When `/` is pressed and no search filter is
visible, it opens the section-level `SearchInput` and focuses it.
Once the `SearchInput` has focus, subsequent `/` keystrokes type into
the field (suppressed by rule 1). `Escape` clears the filter, blurs
the input, and reactivates single-key shortcuts.

**`Escape` priority chain:**

`Escape` is handled by the innermost active context, checked in order:

1. Close open modal (`role="dialog"`)
2. Close sidebar overlay (if open, <1024px viewport)
3. Clear section search filter (if active) and blur input
4. Collapse expanded item detail (if an item row has
   `aria-expanded="true"` and focus is inside it)
5. No-op if none of the above apply

Only ONE action fires per `Escape` press. This is a priority chain,
not a sequence.

---

### 4. Responsive Contract: Below 1024px

**Pattern chosen: Sidebar overlay** (matching the Go report).

Horizontal tabs were considered but rejected: 12 sections do not fit
in a horizontal tab bar without overflow handling, and adding an
overflow menu reintroduces the same progressive-disclosure problem the
sidebar solves. The Go report's overlay pattern is proven, accessible,
and already familiar to users of the current tool.

**Breakpoints:**

| Viewport width | Layout behavior |
|----------------|-----------------|
| >= 1280px | Full three-zone layout (sidebar + main + Containerfile panel) |
| < 1280px | Containerfile panel auto-collapses to 28px vertical tab. Two-zone layout (sidebar + main). |
| < 1024px | Sidebar hidden by default. Hamburger button in masthead. Sidebar renders as overlay. Containerfile panel collapsed. |

**Sidebar overlay specification (< 1024px):**

**Trigger:** Hamburger button in the masthead (`<header>`), left side.

```html
<button type="button"
        aria-label="Open navigation"
        aria-expanded="false"
        aria-controls="sidebar">
  &#x2630;
</button>
```

**Open behavior:**

1. `aria-expanded` flips to `"true"` on the hamburger button.
2. `aria-label` changes to `"Close navigation"`.
3. Sidebar renders as a fixed-position overlay on the left edge,
   over a semi-transparent backdrop (`rgba(0,0,0,0.5)`).
4. Focus moves to the first nav link inside the sidebar.
5. Focus trap activates: `Tab`/`Shift+Tab` cycle within the sidebar.
   Focus cannot escape to content behind the overlay.
6. `Escape` closes the overlay.
7. Clicking the backdrop closes the overlay.

**Close behavior:**

1. Sidebar overlay and backdrop are hidden.
2. `aria-expanded` flips to `"false"`.
3. `aria-label` reverts to `"Open navigation"`.
4. Focus returns to the hamburger button.
5. Focus trap is removed.

**Section activation from overlay:**

When the operator activates a nav link inside the overlay:

1. The overlay closes (same close behavior as above).
2. Focus moves to the main content area -- specifically to the first
   item in the selected section (see focus state machine, section 6).
3. The hamburger button does NOT receive focus in this case -- the
   operator's intent was to navigate, not to return to the masthead.

**What happens to global search (< 1024px):**

The global `SearchInput` remains inside the sidebar overlay. It is
accessible only when the overlay is open. This is acceptable because
section-level search (`/`) remains available at all times. Global
search is a convenience for cross-section discovery, not the primary
search path.

**What happens to badges:**

Badges on NavItems remain visible inside the overlay. They are not
mirrored in the hamburger button or the masthead. The stats bar
(always visible, full-width) provides the same summary information.

**What happens to host info:**

Host info remains at the bottom of the sidebar overlay. It is not
promoted to the masthead. It is orientation context, not
moment-to-moment reference -- visible when the operator opens
navigation is sufficient.

---

### 5. Search Reveal and Focus Contract

#### 5a. Section-Level Search (`/`)

**Activation:** `/` opens an inline `SearchInput` above the active
section's item list. Focus moves to the input. The input is persistent
once opened -- it does not auto-close after a search.

**Filtering:** Real-time as the operator types. Filters by item name,
path, and attention reason. Items that do not match are removed from
the grid (not hidden with CSS -- removed from the roving tabindex
pool). The grid's `aria-rowcount` updates.

**Result in a collapsed attention group:**

When the filter matches items inside a collapsed `AttentionGroup`
(Informational or Routine), the group auto-expands. The group header
updates its count to reflect filtered results. If the filter is
cleared, the group returns to its previous collapsed/expanded state.

Implementation detail: Track a `filterForceExpanded` flag per group.
When filtering force-expands a group, set the flag. When the filter is
cleared, collapse groups that have `filterForceExpanded` set, unless
the operator manually expanded them during filtering.

**Entering results from search:** `ArrowDown` from the search input
moves focus to the first matching item (first row with `tabindex="0"`
in the first visible grid). `ArrowUp` from the first item returns to
the search input.

**Clearing:** `Escape` clears the filter text, restores the full item
list, collapses any groups that were force-expanded by filtering, and
blurs the search input. Focus moves to the first item in the
NeedsReview grid (or the section heading if no items exist).
Single-key shortcuts reactivate.

**Zero results:** `EmptyState` component with message "No packages
matching '{query}'" and a "Clear filter" button. The button is
focusable. Activating it has the same effect as pressing `Escape`.

#### 5b. Global Search (Sidebar)

**Activation:** The global `SearchInput` is always visible in the
sidebar (or inside the sidebar overlay on narrow viewports). It is
focusable via Tab. There is no single-key shortcut to focus it -- `/`
always targets the section-level search.

**Searching:** Searches across ALL sections (both Decisions and
Context) by item name and path. Results render as a dropdown list
below the input, each result showing the item name and the section it
belongs to.

**Selecting a result:**

1. `ArrowDown`/`ArrowUp` navigate the result dropdown. `Enter`
   selects.
2. The sidebar navigates to the result's section (equivalent to
   clicking that section's NavItem).
3. If the result is in a collapsed attention group, the group
   auto-expands.
4. The item receives a temporary visual highlight: a
   `--pf-t--global--color--status--info` left-border pulse that
   persists for 3 seconds or until the next user interaction
   (keypress, click, or scroll), whichever comes first.
5. The item is scrolled into view
   (`scrollIntoView({block: "nearest"})`).
6. Focus lands on the highlighted item's row (it becomes the
   `tabindex="0"` row in its grid).
7. Any existing section-level filter is cleared. Rationale: the
   operator used global search to find a specific item. Keeping an
   unrelated filter active would hide the item they just navigated to.
8. The global search input is cleared and the result dropdown closes.
9. If the sidebar is in overlay mode (< 1024px), the overlay closes
   before navigating to the result.

**Result in a different section:**

When the result is in a section other than the currently active one,
the active section changes (sidebar `aria-current` updates), the new
section renders, and steps 3-8 above apply. No intermediate loading
state is needed -- section data is already fetched (actionable data
from `/api/view`, informational data from `/api/snapshot/sections`).

**Result in an informational section:**

When the result is in a Context section (Services, Containers, etc.),
the section renders as an `InformationalSection` (DataList, no
toggles). The item receives the same temporary highlight. Focus lands
on the matching DataList item. If the item is expandable (e.g., a
service with unit file detail), it is NOT auto-expanded -- the
highlight indicates location, the operator decides whether to expand.

**Highlight behavior:**

- CSS class: `item--search-highlight`
- Visual: animated left-border color transition (standard -> info-blue
  -> standard over 3 seconds). Respects `prefers-reduced-motion` --
  if reduced motion, use a static info-blue border for 3 seconds,
  then remove.
- Cleared on: timeout (3s), any keypress, any click, any scroll event
  on the main content area.
- Only one highlight active at a time. A new search result selection
  replaces any existing highlight.

---

### 6. Focus Management State Machine

Every user action that changes the visible content has a defined focus
destination. If an action is not listed below, focus does not move.

Notation: "-> first item in section" means the first row in the first
visible grid within that section (NeedsReview grid if it has items,
else Informational grid, else Routine grid). If the section has no
items, focus moves to the section heading (`<h2>`).

| Action | Focus destination |
|--------|-------------------|
| Sidebar section change (click NavItem or `1`-`9`) | -> first item in the newly active section |
| Toggle item (Space/x on row, or Switch activation) | Stays on current row. No movement. |
| Undo (`Ctrl+Z`) | -> the affected item's row. If the item is not visible (wrong section active, or in a collapsed attention group), expand the group if in the active section, or navigate to the item's section first. If the item was hidden by a filter, clear the filter then focus the item. If the item cannot be located (e.g., removed from API response), -> section heading. |
| Redo (`Ctrl+Shift+Z`) | Same rules as undo. |
| Search result selection (global) | -> the matched item's row in the target section (see section 5b step 6). |
| Section search ArrowDown | -> first matching item in the first visible grid. |
| Section search Escape (clear filter) | -> first item in the NeedsReview grid (if it has items), else section heading. |
| Modal close (Export, Shortcut overlay, File viewer) | -> the element that triggered the modal. Store `returnFocusEl` on modal open. |
| Containerfile panel collapse (Ctrl+E or chevron) | Focus does not move. The panel collapse is a layout change, not a navigation action. |
| Containerfile panel expand (Ctrl+E or vertical tab) | Focus does not move. Same rationale. |
| Attention group expand (click header) | -> first item in the group's grid. |
| Attention group collapse (click header) | -> the group header button. |
| Filter clear button (in EmptyState) | Same as section search Escape. |
| Sidebar overlay open (hamburger) | -> first nav link in the sidebar. |
| Sidebar overlay close (Escape / backdrop) | -> hamburger button. |
| Sidebar overlay section activation | -> first item in the selected section (NOT the hamburger button). |
| Error toast appears | Focus does not move. The toast is announced via `aria-live`. |
| API error reverts toggle | Focus stays on the item whose toggle was reverted. The error toast is announced via `aria-live`. |

**General rules:**

- Focus destinations that target an item row always set that row's
  `tabindex="0"` and call `.focus()` on it.
- When focus must move to an item that is below the visible scroll
  area, call `scrollIntoView({block: "nearest"})` BEFORE `.focus()`.
- `aria-live="polite"` regions announce state changes (toggle results,
  error messages) without moving focus. Announcements are debounced at
  300ms to prevent chatter during rapid toggles.
- If `prefers-reduced-motion` is active, skip scroll animation -- use
  `behavior: "auto"` instead of `"smooth"`.

---

### 7. Live Regions and Announcements

Two `aria-live="polite"` regions exist, both visually hidden
(`.sr-only`):

1. **Toggle announcements:** Announces include/exclude state changes.
   Format: `"{ItemName} excluded"` or `"{ItemName} included"`.
   Debounced at 300ms -- if multiple toggles fire within 300ms, only
   the last announcement is spoken.

2. **Stats announcements:** Announces stats bar updates. Format:
   `"{N} of {M} remaining"` (only when the remaining count
   changes). Same 300ms debounce. Separate from toggle announcements
   so they do not stomp each other.

Error toasts (`Alert`) use their own `aria-live` via PatternFly's
`AlertGroup` component. These are not debounced -- errors are
infrequent and each one matters.

---

### 8. Visual Accessibility

- All interactive elements have visible focus indicators:
  `:focus-visible` outline (PatternFly default, 2px offset ring).
- `prefers-reduced-motion`: disable CSS transitions on panel
  collapse/expand and attention group expand/collapse. Use instant
  show/hide. Disable search-highlight animation -- use static border.
  Disable `scrollIntoView` smooth scrolling.
- Touch targets: all toggle switches and expand chevrons meet 44x44px
  minimum tap target (WCAG 2.2 AA, Success Criterion 2.5.8).
- Color is never the sole indicator: attention levels have text labels
  (`Label` component) alongside border colors. Toggle state has text
  ("Included"/"Excluded") alongside the switch position. Review
  progress has text alongside the progress bar.
- Skip link at page top: `<a class="pf-v6-c-skip-to-content__link"
  href="#main-content">Skip to main content</a>`. Target is the
  active section's heading.
- Three landmark regions: `<nav aria-label="Section navigation">`,
  `<main>`, `<aside aria-label="Containerfile preview">`. Screen
  reader landmark navigation (e.g., `D` in NVDA) moves between them.
- Shortcut overlay (`?`) is a `Modal` with `role="dialog"`,
  `aria-modal="true"`, and focus trap (Tab cycles within the modal).
  Close button is the first focusable element.

## States

### Loading

- Initial page load: `Skeleton` components in main content area and
  Containerfile panel
- Stats bar shows placeholder dashes until first `/api/view` response
- Sidebar renders immediately (section list is static)

### Empty States

- Section has zero items: `EmptyState` — "No config files detected on
  this host"
- Filter produces zero matches: `EmptyState` — "No packages matching
  'nginx'" with "Clear filter" action
- All NeedsReview items triaged: attention group header updates to
  "All triaged ✓" with success styling

### Error States

- **API unreachable** (server died): Full-page error with "Server
  connection lost" message and retry button
- **Operation failure** (POST /api/op error): Transient `Alert` toast,
  toggle reverts to previous state, 3-second auto-dismiss
- **Stale export** (generation mismatch 409): `Alert` explaining the
  view has changed since last review, auto re-fetches `/api/view`
- **Error boundaries:** Main content and Containerfile preview have
  independent error boundaries. A Containerfile render failure does
  not take down the decision interface.

### Responsive

| Breakpoint | Behavior |
|------------|----------|
| >= 1280px | Full three-zone layout |
| < 1280px | Containerfile panel auto-collapses to vertical tab |
| < 1024px | Sidebar overlay (hamburger trigger). See Interaction / Accessibility Contract section 4 for full specification. |

## Informational Sections

Informational sections display data from `GET /api/snapshot/sections`.
They appear under the "Context" group in the sidebar, collapsed by
default.

### Rendering

- PatternFly `DataList` without selection — flat list, muted styling
- No toggle switches, no attention sub-grouping
- Muted gray left-border (vs. colored borders on actionable items)
- Expand/collapse detail on individual items where relevant (e.g.,
  service unit files, container details)

### Section Order (by signal value)

1. Services — directly informs package decisions
2. Containers — quadlets and compose files appear in Containerfile
3. Users & Groups — sudoers, SSH keys, password state
4. Non-RPM Software — pip, npm, manual installs
5. System Info — OS release, system type, hardware
6. Kernel Boot — kernel params, modules
7. Storage — fstab, mounts, LVM
8. Network — NM connections, firewall, routes
9. SELinux — policy state, booleans, modules
10. Scheduled Tasks — cron jobs, systemd timers

### Containerfile Relationship

Items from informational sections (quadlets, compose files, flatpaks)
that affect the build DO appear in the Containerfile preview. The
operator can see them in the preview but cannot toggle them in v1.
This is by design — the "Context" sidebar group exists partly to help
operators understand what they see in the Containerfile.

## PatternFly Component Map

| UI Element | PF Component | Notes |
|------------|-------------|-------|
| Overall layout | `Page`, `PageSection` | |
| Sidebar | `Nav`, `NavGroup`, `NavItem` | With `Badge` for counts |
| Section search | `SearchInput`, `Toolbar` | |
| Attention groups | `ExpandableSection` | With count in header |
| Decision cards | Custom card (div) | PF cards too heavy for density |
| Toggle switch | `Switch` | Include/exclude |
| Attention labels | `Label` | Color-coded by reason |
| Badges | `Badge` | Sidebar counts |
| Undo/Redo | `Button` (secondary) | With Tooltip |
| Export | `Button` (primary) | Separated in stats bar |
| Containerfile | `CodeBlock` (read-only) | With syntax highlighting |
| Export confirm | `Modal` | Summary + generation |
| Error toasts | `Alert`, `AlertGroup` | Transient, auto-dismiss |
| Empty states | `EmptyState` | Zero items, zero filter results |
| Loading | `Skeleton` | Initial load |
| Shortcut overlay | `Modal` | Keyboard shortcut reference |
| Informational lists | `DataList` | Without selection |
| Host info | Custom component | Bottom of sidebar |

## Project Structure

```
inspectah-web/
  ui/
    package.json
    vite.config.ts
    tsconfig.json
    index.html
    src/
      main.tsx                    ← React entry point
      App.tsx                     ← Root component, state, routing
      api/
        client.ts                 ← API client (fetch wrappers)
        types.ts                  ← TypeScript types matching Rust serde
      components/
        layout/
          StatsBar.tsx
          Sidebar.tsx
          ContainerfilePanel.tsx
          PageLayout.tsx
        sections/
          ActionableSection.tsx
          InformationalSection.tsx
          AttentionGroup.tsx
        items/
          DecisionCard.tsx
          CompactRow.tsx
          DataListItem.tsx
        common/
          SearchFilter.tsx
          SearchGlobal.tsx
          ExportModal.tsx
          ShortcutOverlay.tsx
          ErrorBoundary.tsx
      hooks/
        useRefineApi.ts           ← API calls, optimistic updates
        useKeyboardShortcuts.ts   ← Global + section keyboard handlers
        useLocalStorage.ts        ← Panel state persistence
      styles/
        overrides.css             ← PF token overrides if needed
```

## Design Decisions Log

| Decision | Choice | Rationale | Alternatives considered |
|----------|--------|-----------|------------------------|
| Primary grouping axis | Item type (Packages, Configs) | Sysadmins think in item domains, not attention levels. Reduces context-switching. Scales to future sections. | Attention-level-first (mixes item types) |
| Interaction pattern | Grouped panels with triage behavior on NeedsReview | Combines guided focus (triage) with full-picture exploration (panels). Expert users need both. | Pure triage queue (blocks exploration), filterable list (no guidance) |
| Containerfile panel | Persistent collapsible right panel | Always-visible for decision verification. Collapses to reclaim space for config diffs. Not a Drawer (wrong PF semantic). | Bottom panel (competes with list height), overlay drawer (covers content) |
| Informational sections | Read-only flat lists under "Context" sidebar group | Provides host context without actionable confusion. Separate API endpoint avoids polluting RefinedView. | Summary only (insufficient), defer to post-v1 (loses context) |
| ARIA semantics | role="grid" + role="row" with nested switch in gridcell | Items have three behaviors (toggle, expand, navigate). Grid is the correct composite widget for managed focus with interactive children. role="list" is structural, not composite (no roving tabindex). role="listbox" forbids interactive descendants. See Interaction / Accessibility Contract section 2. | role="list" + role="listitem" (not a composite widget), role="listbox" + role="option" (forbids interactive children) |
| Focus after toggle | Stay on current item | Expert users scan at their own pace. Auto-advance disrupts. | Auto-advance to next NeedsReview item |
| Focus after undo/redo | Return to affected item | Undo from a different scroll position is disorienting without this. Falls back to section header if item not visible. | Stay at current position |
| Frontend framework | React 19 + Vite | PatternFly has official React components. Kit has React experience. Vite builds to static files for rust-embed. | Vanilla JS (tedious reactivity), Alpine.js (too light), Svelte (smaller ecosystem) |
| State management | React state (no Redux/Zustand) | API is source of truth. Optimistic updates are simple. No cross-component state sharing complex enough to warrant a library. | Redux (overkill), Zustand (unnecessary) |
| Export placement | Stats bar, visually separated | Terminal action needs discoverability but must not blend with status counters. | Toolbar menu (hidden), floating button (out of context) |

## Browser Trust Contract

The refine UI renders content from inspection snapshots. Snapshots
capture arbitrary file contents, config diffs, unit files, firewall
rules, and other strings from a live host. None of this content is
sanitized at the Rust layer — it arrives as-is from the filesystem.
This section establishes the rules that keep the browser safe and the
operator honestly informed.

### 1. Text-Only Rendering Rule

**All snapshot-derived strings MUST render as text nodes, never as
HTML.** This is the single most important security rule in the frontend.

**What counts as snapshot-derived:** Any value originating from the
inspection tarball, delivered via `/api/view` or `/api/snapshot/sections`.
This includes 32 string fields across 13 types:

High-risk fields (file content, shown in code blocks or diff views):
- `ConfigFileEntry.content`, `ConfigFileEntry.diff_against_rpm`
- `SystemdDropIn.content`, `QuadletUnit.content`
- `NonRpmItem.content`, `CarryForwardFile.content`
- `FirewallZone.content`

Medium-risk fields (names, paths, metadata shown in labels and lists):
- Names: `*.name`, `*.unit`, `*.image`, `*.version`
- Paths: `*.path`, `*.source`
- Free text: `NonRpmItem.notes`, `NonRpmItem.review_status`
- Rules: `FirewallZone.rich_rules`, `SelinuxSection.fcontext_rules`
- Raw lines: `ProxyEntry.line`, `ConfigFileEntry.rpm_va_flags`

**Implementation rules:**

1. **No `dangerouslySetInnerHTML` for any snapshot-derived content.**
   This is an absolute ban, not a guideline. Grep the codebase for
   `dangerouslySetInnerHTML` in CI — any occurrence adjacent to API
   data is a build-breaking violation.

2. **React JSX escaping is sufficient for most surfaces.** When
   snapshot strings are passed as JSX children (`<span>{item.name}</span>`)
   or prop values (`title={item.path}`), React escapes them
   automatically. This covers decision cards, compact rows, sidebar
   labels, search results, DataList items, and stats bar counts.

3. **Syntax highlighting must operate on text content.** The
   Containerfile preview uses a `CodeBlock` with Dockerfile grammar
   highlighting (Prism.js or highlight.js). The highlighter receives
   the Containerfile string as a text input and produces tokenized
   spans with CSS classes. It must NOT receive pre-formed HTML.
   Implementation pattern:
   ```tsx
   // CORRECT: text in, styled spans out
   <CodeBlock code={containerfile} language="dockerfile" />

   // WRONG: never inject highlighted HTML
   <div dangerouslySetInnerHTML={{__html: highlight(containerfile)}} />
   ```
   If the chosen highlighting library's React wrapper requires
   `dangerouslySetInnerHTML` internally (some do), that internal use
   is acceptable ONLY if the library is processing the raw text string
   through its own tokenizer — not receiving pre-formed HTML from our
   code. Document which library was chosen and verify this property.

4. **Config diffs must render via text nodes with CSS classes.** The
   `diff_against_rpm` field contains unified diff strings. Diff
   rendering must split on newlines and apply CSS color classes
   (`diff-add`, `diff-remove`, `diff-context`) to `<span>` elements
   containing text nodes. Do not use an HTML diff library that produces
   `<ins>`/`<del>` tags via innerHTML injection. Pattern:
   ```tsx
   {diffLines.map((line, i) => (
     <span key={i} className={classForLine(line)}>
       {line}
     </span>
   ))}
   ```

5. **Informational section detail expansion follows the same rules.**
   When an operator expands a service drop-in, quadlet unit, or
   firewall zone to view its content, that content renders as a
   `CodeBlock` or `<pre>` with text nodes. Same constraint as the
   Containerfile preview.

### 2. Export Trust Signaling

The export confirmation dialog must honestly represent what the
operator controls and what they do not.

**Problem:** The current export dialog shows "X packages excluded, Y
configs excluded." This implies the operator reviewed and decided on
everything in the tarball. In reality, the exported tarball also
contains content from informational context sections — services,
containers, quadlets, flatpaks, SELinux policy, network config, and
others — that appear in the generated Containerfile but cannot be
toggled in v1. The operator saw these items in the Context sidebar
but had no include/exclude control over them.

**Revised export confirmation dialog:**

```
+---------------------------------------------------------+
|  Export Refined Tarball                                  |
|                                                         |
|  Your decisions:                                        |
|    142 packages included, 8 excluded                    |
|    23 config files included, 2 excluded                 |
|                                                         |
|  +---------------------------------------------------+  |
|  | [!] This tarball also includes services,           |  |
|  | containers, and other context sections that        |  |
|  | appear in the Containerfile but are not yet        |  |
|  | individually toggleable. Review the Containerfile  |  |
|  | preview before exporting.                          |  |
|  +---------------------------------------------------+  |
|                                                         |
|  Generation: 14                                         |
|                                                         |
|             [ Cancel ]    [ Export ]                     |
+---------------------------------------------------------+
```

The warning uses a PatternFly `Alert` component (`variant="warning"`,
inline) inside the `Modal`. It is always visible — not a dismissable
banner, not a tooltip. The operator cannot miss it.

**Containerfile preview persistent indicator:** The Containerfile
preview panel footer should include a static note below the line
count: "Includes context sections (not toggleable in v1)". This is
always visible when the panel is open, not just at export time. It
uses `--pf-t--global--color--status--info` text color and a small
font size. This helps operators understand what they are looking at
while making decisions, not only when they export.

**Rationale:** This is not about blocking the export — the operator
is the authority on their host. It is about preventing a false sense
of completeness. "I reviewed everything in this tarball" should be
true when the operator says it, and in v1, it is not fully true for
context sections. The UI should say so plainly.

### 3. Content Security Policy

The axum server must set a `Content-Security-Policy` header on all
HTML responses. This is defense-in-depth — even if a rendering bug
allows script injection, CSP prevents execution.

**Recommended CSP header value:**

```
Content-Security-Policy:
  default-src 'none';
  script-src 'self';
  style-src 'self' 'unsafe-inline';
  img-src 'self' data:;
  font-src 'self';
  connect-src 'self';
  frame-ancestors 'none';
  base-uri 'none';
  form-action 'self'
```

**Directive rationale:**

| Directive | Value | Why |
|-----------|-------|-----|
| `default-src` | `'none'` | Deny-by-default. Every resource type must be explicitly allowed. |
| `script-src` | `'self'` | All JS is embedded via rust-embed. No CDN, no inline scripts. Vite's build output is static `.js` files served from `/assets/`. |
| `style-src` | `'self' 'unsafe-inline'` | PatternFly 6 injects inline styles for dynamic component sizing (popovers, tooltips, modals). `'unsafe-inline'` is required for PF compatibility. If PF moves to CSS custom properties only in a future version, drop `'unsafe-inline'`. |
| `img-src` | `'self' data:` | `data:` URIs for PatternFly's inline SVG icons. No external images. |
| `font-src` | `'self'` | Red Hat fonts embedded via rust-embed. No Google Fonts or external CDN. |
| `connect-src` | `'self'` | API calls to the same origin only. No external API calls. |
| `frame-ancestors` | `'none'` | The refine UI must never be embedded in an iframe. Prevents clickjacking. |
| `base-uri` | `'none'` | Prevent `<base>` tag injection that could redirect relative URLs. |
| `form-action` | `'self'` | The UI has no forms, but this prevents any injected form from submitting to an external target. |

**Implementation:** Add the CSP header in the axum middleware layer,
applied to the `serve_report` response (the HTML page). Static asset
responses (JS, CSS, fonts) do not need CSP headers.

```rust
// In assets.rs, on the index.html response:
.insert_header(
    "Content-Security-Policy",
    "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; font-src 'self'; connect-src 'self'; \
     frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
)
```

**Why `'unsafe-inline'` for styles is acceptable here:** The threat
model for this UI is snapshot content injection, not a hostile network
attacker rewriting HTML. The snapshot data enters via JSON API responses
and is rendered as text nodes (see rule 1). CSS injection via inline
styles is not a realistic attack vector for this application. The
`style-src 'unsafe-inline'` is a pragmatic concession to PatternFly's
implementation, not a security gap in context.

### 4. Self-Hosted Assets Rule

**All frontend assets must be embedded in the binary via rust-embed.
No CDN references. No external resource loading.**

This is already implied by the architecture (Vite builds to `dist/`,
rust-embed serves from `static/`), but it must be an explicit,
checkable rule:

- PatternFly CSS and JS: bundled by Vite, served from `/assets/`
- Syntax highlighting library (Prism.js or highlight.js): bundled
  by Vite, served from `/assets/`
- Red Hat fonts: included in the Vite build, served from `/assets/`
- SVG icons: bundled inline or as static assets, never fetched from
  an icon CDN

**Verification:** The CSP `connect-src 'self'` and `script-src 'self'`
directives enforce this at runtime. A browser console error on any
external fetch is a signal that the self-hosting rule was violated.
CI should also grep built HTML/JS for `https://` URLs that are not
in code comments — any match is a build failure.

**Rationale:** inspectah runs on hosts that may not have internet
access. The refine server binds to localhost. External resource
loading would break offline use and leak information about the
operator's activity to third parties. The embedded binary must be
fully self-contained.
