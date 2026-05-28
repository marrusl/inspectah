# Phase 2 Slice 2b: Expansion Inspectors Implementation Plan

**Date:** 2026-05-13 (revised)
**Scope:** Network, Containers, Users/Groups inspectors
**Branch:** `rust`
**Baseline:** Slice 2a code-complete — 339 tests, 7 inspectors (RPM + services + storage + kernelboot), three-wave parallel execution, CI green. Host validation evidence for 2a is finalized in this slice's Task 9 alongside the 2b sections.

## Prerequisites

- [ ] Slice 2a merged and pushed to `rust` branch
- [ ] `cargo test --workspace` passes (~339 tests)
- [ ] `cargo clippy --workspace -- -W clippy::all` clean
- [ ] All three new inspectors are Wave 1 (no RPM dependency) — they slot into the existing parallel execution with zero orchestration changes

## Three Proof Lanes

This plan maintains three separate proof lanes, matching the pattern established in Slice 2a:

| Lane | Location | What it proves |
|------|----------|----------------|
| **Serde/golden roundtrip** | `inspectah-core/tests/parity_gate.rs` | Rust types can deserialize Go-captured golden JSON and re-serialize without loss |
| **Inspector-on-fixture** | `inspectah-collect/tests/{network,containers,users}_test.rs` | Rust inspectors produce correct output from fixture data via MockExecutor |
| **Live-host closure** | `testdata/evidence/`, `scripts/host-validation.sh` | Real Go+Rust scans match on a live system |

Each new section (network, containers, users_groups) MUST have entries in all three lanes. The lanes are NOT interchangeable — passing one does not excuse skipping another.

### Normalizer note

`inspectah-core/src/normalize.rs` strips `redaction_state` and `completeness` from both sides during parity comparison. These are Rust-only trust-bearing fields that Go does not produce. The normalizer strips them so the comparison works, but Task 9's host validation evidence must separately prove these fields are populated correctly by checking the raw Rust JSON output before normalization.

## Success Criteria

1. Three new inspectors implemented: `NetworkInspector`, `ContainersInspector`, `UsersGroupsInspector`
2. All three implement the `Inspector` trait with `fn applicable_to(&self) -> &[SourceSystemKind]` returning `&[SourceSystemKind::PackageBased]`
3. Each inspector has 15–25 unit tests in-module, plus integration tests in `inspectah-collect/tests/`
4. Sensitive input handling verified:
   - `/etc/shadow`: expiry/status only, hash NEVER stored. `shadow_entries` stores **stripped lines** (username + expiry fields only, hash field replaced with status indicator). Negative test proves no `$6$`/`$y$`/`$5$` hash prefix appears in snapshot JSON.
   - `/etc/gshadow`: stores admin/member lists only. Hash/password field stripped on parse, same as shadow. `gshadow_entries` contains `group:!:admins:members` format (hash replaced with `!` placeholder).
   - `~/.ssh/authorized_keys`: presence/count only, not key content
   - Sudoers: rules persisted, redaction engine scans for embedded passwords
   - Proxy env vars: network inspector stores raw proxy URLs in `proxy[].line` (including any embedded credentials). A dedicated `mask_proxy_credentials()` function (~30-40 lines in `inspectah-pipeline/src/redaction/engine.rs`) handles proxy-URL credential masking. This cannot use `redact_string()` because that function does whole-match replacement via `find_iter()` + `token_for()` and cannot produce partial masking like `http://user:[REDACTED]@host`. The dedicated function uses capture-group-aware replacement: it matches `://user:password@host` patterns, captures the password segment specifically, and replaces ONLY the password with `[REDACTED]`, preserving the rest of the URL structure (`http://admin:[REDACTED]@proxy.corp.com:8080`). It handles edge cases: passwords with special chars, multiple `@` signs, port numbers. The `mask_proxy_credentials()` function generates `RedactionFinding` entries directly when it detects and masks a credential — no shared `SecretPattern` in `patterns.rs` is needed. The generic `redact_string()` pass does not process `network.proxy[].line` fields; the dedicated masker is the sole owner. Containerfile comments show the masked URL. The secrets-review artifact flags the finding.
   - Compose env vars: inspector emits `RedactionHint`s for suspicious env var names found during compose file scanning. No compose content/env field is persisted — `ComposeFile` stores path + images only. Schema is additive-ready: `Option<Vec<ComposeEnvVar>>` field with `#[serde(default)]` can be added later as a non-breaking change.
   - `podman inspect` environment: redaction engine scans persisted `running_containers[].env[]` entries
5. Two-strategy auto-detect provisioning model implemented: `sysusers` (no valid login shell) and `blueprint` (valid login shell). Override-only strategies (`useradd`, `kickstart`) accepted as an internal inspector option for unit test coverage. CLI flag `--user-strategy-override` is deferred — override ingress is not budgeted in Slice 2b. Deliberate divergence from Go's three-way classification documented in `testdata/divergences.md`.
6. Parity gate covers 7 sections cumulative (RPM + services + storage + kernelboot + network + containers + users_groups)
7. Renderer smoke tests passing for all 3 new sections against existing consumers only (containerfile, configtree, kickstart, readme). Audit/report rendering for these sections is **deferred** to a later slice.
8. Failure policy tested: degraded for parse errors, PermissionDenied; silent skip for NotFound
9. Redaction engine extended for new persisted surfaces with planted-secret proofs
10. Host validation on same CentOS Stream 9 box using the real Rust CLI (`inspectah-cli scan`), closing BOTH 2a and 2b evidence in one pass across all 7 sections
11. Test count target: ~339 (Slice 2a baseline) + ~74 new = ~413+
12. All commits follow conventional commit format
13. Clippy clean, `cargo fmt` clean

## Dependency & Parsing Design Decisions

### XML parsing for firewalld zones

The Go inspector uses `encoding/xml` for zone parsing. In Rust, use a **lightweight hand-parser** rather than adding a crate dependency:
- firewalld zone XML has a simple, well-defined schema: `<zone>` with `<service>`, `<port>`, and `<rule>` children
- Extract service names, port/protocol pairs, and rich rule text via simple string scanning
- This matches the Go `extractRichRules()` approach for rich rules and keeps dependencies minimal
- If the XML is malformed, return empty results with Degraded status — not a panic or silent skip. Explicit test required for malformed input.

### YAML parsing for compose files

The Go inspector uses **no YAML library** — it extracts `services:` blocks and `image:` keys via line-by-line scanning with indent detection (`extractComposeImages`). Follow the same approach in Rust:
- Detect `services:` top-level key
- Calibrate indent from first service key
- Extract service names and image references
- No `serde_yaml` dependency needed
- If the YAML is malformed or uses unsupported structure, return empty results with Degraded status — not a panic or silent skip. Explicit test required for malformed input.

### JSON parsing for podman output

Use `serde_json` (already a workspace dependency) for:
- `podman ps --format json` output
- `podman inspect` output
- Both are parsed as `Vec<serde_json::Value>` and fields extracted dynamically, matching the Go pattern

### `regex` dependency scope

If `regex` is needed for compose pattern matching, add it as a dependency of `inspectah-collect` only (`inspectah-collect/Cargo.toml`), NOT workspace-wide in the root `Cargo.toml`.

### User classification and two-strategy auto-detect provisioning

**Deliberate Go divergence.** The Go inspector uses a three-way classification (service/human/ambiguous) with four strategy paths. Mark has decided the Rust implementation simplifies this to a **two-category auto-detect model** based solely on login shell:

| Shell type | Auto-assigned strategy | Artifact | Slice 2b status |
|-----------|----------------------|----------|-----------------|
| No valid login shell (`/sbin/nologin`, `/bin/false`, `/usr/sbin/nologin`, or unknown/other) | `sysusers` | sysusers.d drop-in | **FIXME/deferred** — strategy assigned, sysusers.d materialization deferred. Test asserts FIXME comment. |
| Valid login shell (`/bin/bash`, `/bin/zsh`, `/bin/sh`, `/bin/fish`, `/bin/tcsh`, `/bin/csh`, `/usr/bin/bash`, `/usr/bin/zsh`, `/usr/bin/fish`) | `blueprint` | Image-builder provisioning | **FIXME/deferred** — strategy assigned, blueprint carry-forward deferred. Test asserts FIXME comment. |

The `ambiguous` classification is eliminated. The inspector auto-assigns only `sysusers` or `blueprint` — no other strategies are auto-detected.

**Override-only strategies:** `useradd` and `kickstart` are NEVER auto-assigned. They are available only through an internal inspector option (matching Go's `UserGroupOptions.UserStrategyOverride`). CLI flag `--user-strategy-override` is deferred — override ingress is not budgeted in Slice 2b. The inspector accepts the option internally for unit test coverage.

When an override is set internally:
- `useradd` → `RUN useradd` in Containerfile (renderer already handles this)
- `kickstart` → `user` command in kickstart-suggestion.ks (renderer already handles this)

**Classification logic (replaces Go's `classifyUser()`):**

```rust
fn classify_user(user: &serde_json::Value) -> &'static str {
    let shell = user.get("shell").and_then(|v| v.as_str()).unwrap_or("");
    if VALID_LOGIN_SHELLS.contains(&shell) {
        "blueprint"
    } else {
        "sysusers"
    }
}
```

This is simpler than Go's multi-factor classification (shell + home directory + UID). The Go model's `ambiguous` category (unknown shell, /var/ home, etc.) maps to `sysusers` in the new model — if there's no valid login shell, it's a service account.

**Group strategy assignment** follows the primary user's strategy (same as Go's `assignGroupStrategies()` behavior). When no primary user exists for a group, default to `sysusers`. Override mode (explicit strategy set via internal inspector option) is respected when present. CLI flag `--user-strategy-override` is deferred.

This divergence MUST be documented in `testdata/divergences.md` (see Task 7, Step 3).

The Rust `UserGroupSection` uses `Vec<serde_json::Value>` for users/groups — this is deliberate to match Go's `map[string]interface{}` and will carry classification/strategy as JSON fields.

## Artifact-Consumer Matrix

This matrix maps which Slice 2b inspector sections drive which **existing** renderers. Audit (`audit.rs`) and report (`report.rs`) rendering for these sections is deferred to a later slice — this plan does NOT budget or test those consumers.

| Section | containerfile.rs | configtree.rs | kickstart.rs | readme.rs |
|---------|-----------------|---------------|--------------|-----------|
| **network** | `network_section_lines()`: firewall zone count + COPY comment (firewall_only=true), static routes as comments, hosts additions as FIXME comments, proxy config as comments | `write_config_tree()`: materializes firewall zone XML files under `config/etc/firewalld/zones/` | network connections (DHCP→`--bootproto=dhcp`, static→FIXME), hosts additions, static routes, policy rules | Not directly consumed (findings summary counts sections) |
| **containers** | `containers_section_lines()`: `COPY quadlet/ /etc/containers/systemd/` for included quadlets, flatpak COPY + provisioning service enable. **No** compose-image comments or podman-container references — compose/running-container data is informational in snapshot only, no Containerfile rendering in Slice 2b. | `write_config_tree()`: materializes quadlet unit files under `quadlet/` (top-level, NOT under `config/etc/containers/systemd/`). Also writes flatpak manifest (`flatpak/flatpak-install.json`) + provisioning service (`flatpak/flatpak-provision.service`) under `flatpak/`. | Not consumed | Container workload summary: `"{q} quadlet, {c} compose"` counts in findings summary table (quadlet unit count + compose file count). No image or running-container counts. |
| **users_groups** | `users_section_lines()`: sysusers count + config-tree comment, `RUN useradd` for override-useradd users, FIXME for blueprint-strategy users, FIXME referencing kickstart.ks for override-kickstart users. No generic group provisioning lines emitted. | Not consumed for users (but quadlet user dirs derive from passwd) | override-kickstart users → `user` commands in kickstart | Not directly consumed |

**Renderer code changes needed:** The existing renderers already have the section-consuming functions listed above. Task 8 verifies these functions produce correct output when fed Slice 2b inspector data. If any function is missing handling for a sub-field (e.g., a new network field type), the Task 8 step budgets the fix inline. Note: compose/running-container Containerfile rendering is NOT in scope — that data is informational in the snapshot only. No renderer changes needed for compose or podman data in this slice.

## File Map

### New Files

| File | Purpose |
|------|---------|
| `inspectah-collect/src/inspectors/network.rs` | Network inspector implementation |
| `inspectah-collect/src/inspectors/containers.rs` | Containers inspector implementation |
| `inspectah-collect/src/inspectors/users.rs` | Users/Groups inspector implementation |
| `inspectah-collect/tests/network_test.rs` | Network inspector integration tests (inspector-on-fixture proof lane) |
| `inspectah-collect/tests/containers_test.rs` | Containers inspector integration tests (inspector-on-fixture proof lane) |
| `inspectah-collect/tests/users_test.rs` | Users/Groups inspector integration tests (inspector-on-fixture proof lane) |
| `testdata/fixtures/network/eth0.nmconnection` | NM keyfile fixture (INI format) |
| `testdata/fixtures/network/public-zone.xml` | firewalld zone fixture (XML) |
| `testdata/fixtures/network/direct.xml` | firewalld direct rules fixture |
| `testdata/fixtures/network/hosts` | /etc/hosts fixture |
| `testdata/fixtures/network/resolv-nm.conf` | resolv.conf with NM provenance |
| `testdata/fixtures/network/proxy-environment` | /etc/environment with proxy vars |
| `testdata/fixtures/network/ip-route.txt` | ip route output fixture |
| `testdata/fixtures/network/ip-rule.txt` | ip rule output fixture |
| `testdata/fixtures/network/dnf-proxy.conf` | dnf.conf with proxy setting |
| `testdata/fixtures/network/malformed-zone.xml` | Malformed XML for degraded test |
| `testdata/fixtures/network/unsupported-zone.xml` | Valid XML with namespaces/CDATA for degraded test (lightweight hand-parser cannot handle) |
| `testdata/fixtures/containers/webapp.container` | Quadlet .container fixture |
| `testdata/fixtures/containers/webapp-data.volume` | Quadlet .volume fixture |
| `testdata/fixtures/containers/compose.yaml` | docker-compose fixture |
| `testdata/fixtures/containers/compose-malformed.yaml` | Malformed YAML for degraded test |
| `testdata/fixtures/containers/compose-anchors.yaml` | Valid YAML with anchors/aliases for degraded test (lightweight scanner cannot resolve) |
| `testdata/fixtures/containers/podman-ps.json` | podman ps JSON output fixture |
| `testdata/fixtures/containers/podman-inspect.json` | podman inspect JSON output fixture |
| `testdata/fixtures/containers/flatpak-list.txt` | flatpak list output fixture |
| `testdata/fixtures/containers/flatpak-remotes.txt` | flatpak remote-list output fixture |
| `testdata/fixtures/users/passwd` | /etc/passwd fixture |
| `testdata/fixtures/users/shadow` | /etc/shadow fixture (no real hashes) |
| `testdata/fixtures/users/group` | /etc/group fixture |
| `testdata/fixtures/users/gshadow` | /etc/gshadow fixture |
| `testdata/fixtures/users/sudoers` | /etc/sudoers fixture |
| `testdata/fixtures/users/sudoers.d-webapp` | sudoers.d drop-in fixture |
| `testdata/fixtures/users/subuid` | /etc/subuid fixture |
| `testdata/fixtures/users/subgid` | /etc/subgid fixture |
| `testdata/fixtures/users/authorized_keys` | SSH authorized_keys fixture |
| `testdata/golden/go-v13-network-section.json` | Go golden for network section |
| `testdata/golden/go-v13-containers-section.json` | Go golden for containers section |
| `testdata/golden/go-v13-users-groups-section.json` | Go golden for users_groups section |
| `inspectah-pipeline/tests/smoke_render_2b.rs` | Renderer smoke tests for Slice 2b sections |
| `inspectah-pipeline/tests/redaction_2b_surfaces_test.rs` | Redaction planted-secret proofs for new surfaces |

### Modified Files

| File | Change |
|------|--------|
| `inspectah-collect/src/inspectors/mod.rs` | Register `network`, `containers`, `users` modules |
| `inspectah-collect/Cargo.toml` | Add `regex` dependency (scope: `inspectah-collect` only, not workspace-wide) |
| `inspectah-pipeline/src/redaction/engine.rs` | Extend to scan new persisted surfaces (podman env, sudoers). Add `mask_proxy_credentials()` function (~30-40 lines) for capture-group-aware proxy URL credential masking. Compose env handled at inspector time via hints only. |
| `inspectah-pipeline/src/redaction/patterns.rs` | Verify `scan_shadow()` wiring to users inspector output. No new patterns — proxy credential detection is handled entirely by `mask_proxy_credentials()` in `engine.rs`. |
| `inspectah-core/tests/parity_gate.rs` | Expand serde/golden roundtrip tests to network, containers, users_groups sections |
| `inspectah-collect/tests/parity_test.rs` | Expand inspector-on-fixture tests to network, containers, users_groups |
| `scripts/host-validation.sh` | Extend to cover all 7 sections (RPM + services + storage + kernelboot + network + containers + users_groups) |
| `testdata/divergences.md` | Add any new divergence entries |

---

## Task 1: Test Fixtures for All Three Inspectors

**Files:**
- Create: all `testdata/fixtures/network/*`, `testdata/fixtures/containers/*`, `testdata/fixtures/users/*` files listed above

Fixtures are adapted from Go's `cmd/inspectah/internal/inspector/testdata/` with values aligned to the test expectations we will write.

- [ ] **Step 1: Create network fixtures**

  Create `testdata/fixtures/network/` directory with the following files. Content should match the Go test fixture patterns:

  - `eth0.nmconnection`: NM keyfile with `[connection]` type=ethernet, `[ipv4]` method=auto
  - `public-zone.xml`: firewalld zone with `<service name="ssh"/>`, `<port port="443" protocol="tcp"/>`
  - `direct.xml`: firewalld direct rules XML with a sample passthrough rule
  - `hosts`: entries including localhost and a custom host (`10.0.0.50 db.internal`)
  - `resolv-nm.conf`: resolv.conf with `# Generated by NetworkManager` header, nameserver 10.0.0.1
  - `proxy-environment`: `/etc/environment` format with `http_proxy=http://proxy:8080` and `https_proxy=http://user:secret@proxy:8080` (the latter for credential detection tests)
  - `ip-route.txt`: `ip route` output with default gateway and local subnet
  - `ip-rule.txt`: `ip rule` output with local, main, default tables plus one custom rule
  - `dnf-proxy.conf`: `[main]` section with `proxy=http://dnf-proxy:3128`
  - `malformed-zone.xml`: invalid XML content for degraded-handling test
  - `unsupported-zone.xml`: valid XML with features the hand-parser cannot handle — XML namespaces (`<zone xmlns:custom="...">`) and/or CDATA sections (`<![CDATA[...]]>`) wrapping rule text — for valid-but-unsupported degradation test

- [ ] **Step 2: Create container fixtures**

  Create `testdata/fixtures/containers/` directory:

  - `webapp.container`: Quadlet unit with `Image=registry.example.com/webapp:latest`, `PublishPort=8080:80`, `Volume=/data:/app/data:Z`
  - `webapp-data.volume`: Quadlet volume unit
  - `compose.yaml`: docker-compose file with `services:` block, two services with `image:` keys and one service with `environment:` block containing `DB_PASSWORD=hunter2`
  - `compose-malformed.yaml`: YAML with broken indentation for degraded-handling test
  - `compose-anchors.yaml`: valid YAML using anchors/aliases (e.g., `x-defaults: &defaults` with `image: *default_image`) that the lightweight line scanner cannot resolve — for valid-but-unsupported degradation test
  - `podman-ps.json`: JSON array from `podman ps --format json` with one running container
  - `podman-inspect.json`: JSON array from `podman inspect` with Mounts, NetworkSettings, Config.Env (including `API_TOKEN=secret123`), HostConfig.RestartPolicy
  - `flatpak-list.txt`: tab-separated flatpak output with 3 apps (firefox, libreoffice, gimp)
  - `flatpak-remotes.txt`: flatpak remote-list output

- [ ] **Step 3: Create users fixtures**

  Create `testdata/fixtures/users/` directory:

  - `passwd`: non-system users (UID 1000+) with varied shells. Include at least: one with valid login shell (bash → auto-detect blueprint), one with nologin shell (→ auto-detect sysusers), one with unknown/other shell (→ auto-detect sysusers), one with `/bin/zsh` (→ auto-detect blueprint).
  - `shadow`: expiry/status fields only — use locked (`!!`), disabled (`*`), and empty password entries. **No real hashes.** Use `!!` and `*` as the hash field, never `$6$...` or similar.
  - `group`: non-system groups (GID 1000+)
  - `gshadow`: corresponding gshadow entries (use `!` for password field, include admin/member lists)
  - `sudoers`: main sudoers with includes
  - `sudoers.d-webapp`: drop-in with `webapp ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart webapp`
  - `subuid`: subordinate UID mappings for test users
  - `subgid`: subordinate GID mappings for test users
  - `authorized_keys`: sample authorized_keys file (2 keys)

- [ ] **Step 4: Commit**

```bash
git add testdata/fixtures/network/ testdata/fixtures/containers/ testdata/fixtures/users/
git commit -m "test(fixtures): add test fixtures for network, containers, users inspectors"
```

---

## Task 2: Network Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/network.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs`

**Reference:** Go `cmd/inspectah/internal/inspector/network.go`

The network inspector collects NM connections (INI keyfiles), firewalld zones (XML), firewalld direct rules, IP routes, IP rules, hosts additions, proxy settings, DNS provenance, and static route files.

### Go function parity map

| Go function | Rust equivalent | Notes |
|-------------|-----------------|-------|
| `RunNetwork` | `NetworkInspector::inspect()` | Orchestrates all sub-collectors |
| `collectNMConnections` | `collect_nm_connections()` | INI parse of `/etc/NetworkManager/system-connections/*.nmconnection` |
| `classifyConnection` | `classify_connection()` | Extracts method (dhcp/static) and type (ethernet/wifi/etc.) from INI |
| `collectFirewallZones` | `collect_firewall_zones()` | XML parse of `/etc/firewalld/zones/*.xml` |
| `parseZoneXML` | `parse_zone_xml()` | Extract services, ports, rich rules from zone XML |
| `extractRichRules` | `extract_rich_rules()` | String scan for `<rule>...</rule>` elements |
| `collectFirewallDirectRules` | `collect_firewall_direct_rules()` | XML parse of `/etc/firewalld/direct.xml` |
| `detectResolvProvenance` | `detect_resolv_provenance()` | Check resolv.conf comment for NetworkManager/systemd-resolved |
| `collectHostsAdditions` | `collect_hosts_additions()` | Filter non-localhost entries from `/etc/hosts` |
| `collectStaticRoutes` | `collect_static_routes()` | Read route-* and rule-* files from NM connections dir |
| `parseIPRoutes` | `parse_ip_routes()` | Split `ip route` output into non-empty lines |
| `parseIPRules` | `parse_ip_rules()` | Parse `ip rule` output, filter default tables (local/main/default) |
| `collectIPRoutes` | `collect_ip_routes()` | Run `ip route` and `ip rule` commands |
| `collectProxy` | `collect_proxy()` | Scan `/etc/environment`, `/etc/profile.d/*.sh`, env vars for proxy lines |
| `collectDNFProxy` | `collect_dnf_proxy()` | Parse proxy= from `/etc/dnf/dnf.conf` |
| `isProxyLine` | `is_proxy_line()` | Check if a line matches `*_proxy=` or `no_proxy=` pattern |

### Degraded handling

- `PermissionDenied` on `/etc/NetworkManager/system-connections/` or `/etc/firewalld/zones/` → Degraded
- `NotFound` on either directory → silent skip (NM or firewalld not installed)
- XML parse failure on a zone file → skip that zone, Degraded (not panic or silent skip)
- INI parse failure on a keyfile → skip that file, Degraded (not panic or silent skip)
- `ip route` or `ip rule` command failure → warning, continue
- Malformed XML input → explicit Degraded status test required

### Redaction surfaces

- Proxy env vars: the network inspector stores raw proxy URLs in `proxy[].line`. The dedicated `mask_proxy_credentials()` function (added to `engine.rs` in Task 6) uses capture-group-aware replacement to mask only the password segment inline (e.g., `http://admin:[REDACTED]@proxy.corp.com:8080`) and generates `RedactionFinding` entries directly. No shared `SecretPattern` in `patterns.rs` — the dedicated masker is the sole owner of proxy lines. The generic `redact_string()` pass does not process these fields. The inspector also emits `RedactionHint` for proxy lines matching the `user:pass@host` pattern.

- [ ] **Step 1: Register module**

Add `pub mod network;` to `inspectah-collect/src/inspectors/mod.rs`.

- [ ] **Step 2: Implement `NetworkInspector`**

Create `inspectah-collect/src/inspectors/network.rs` implementing the `Inspector` trait:

```rust
pub struct NetworkInspector;

impl Inspector for NetworkInspector {
    fn id(&self) -> InspectorId { InspectorId::Network }
    fn applicable_to(&self) -> &[SourceSystemKind] { &[SourceSystemKind::PackageBased] }
    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        // ... orchestrate sub-collectors
    }
}
```

Key implementation details:
- INI parsing: simple `[section]` + `key=value` line scanner (no crate — same as Go's approach)
- XML zone parsing: string scan for `<service name="..."/>`, `<port port="..." protocol="..."/>`, `<rule>...</rule>` — no XML crate
- `parse_ip_rules()`: filter lines referencing lookup tables `local`, `main`, `default`
- `collect_proxy()`: scan `/etc/environment`, `/etc/profile.d/*.sh` for proxy lines; emit `RedactionHint` for lines matching secret patterns
- `collect_dnf_proxy()`: parse `proxy=` from `/etc/dnf/dnf.conf`
- DNS provenance: check first line of `/etc/resolv.conf` for "NetworkManager" or "systemd-resolved"
- Hosts additions: filter localhost/loopback lines from `/etc/hosts`
- All directory reads use `ctx.executor.read_dir()` / `ctx.executor.read_file()`

- [ ] **Step 3: Write unit tests (16–22 in-module tests)**

Unit tests in `#[cfg(test)] mod tests` within `network.rs`:

1. `nm_connection_dhcp` — INI with method=auto → dhcp
2. `nm_connection_static` — INI with method=manual → static
3. `nm_connection_wifi` — type=wifi detected from `[wifi]` section
4. `nm_malformed_ini_skip_file` — bad keyfile → skip, Degraded
5. `firewall_zone_services_and_ports` — parse services/ports from zone XML
6. `firewall_zone_rich_rules` — extract `<rule>` elements
7. `firewall_zone_malformed_xml_degraded` — bad XML → Degraded status, not panic or silent skip
8. `firewall_zone_valid_but_unsupported_xml_degraded` — valid XML with features the lightweight hand-parser cannot handle (e.g., XML namespaces `<zone xmlns:custom="...">` or CDATA sections `<![CDATA[...]]>` wrapping rule text) → Degraded status, not panic. The hand-parser does not handle namespace prefixes or CDATA unwrapping; it must degrade gracefully rather than silently producing wrong output or panicking.
9. `firewall_direct_rules` — parse passthrough rules from direct.xml
10. `ip_route_parsing` — standard `ip route` output
11. `ip_rule_filtering` — filters local/main/default, keeps custom rules
12. `ip_route_command_failure` — exit code != 0 → warning, empty routes
13. `hosts_additions_filters_localhost` — only non-loopback entries kept
14. `resolv_provenance_networkmanager` — detects NM from comment
15. `resolv_provenance_systemd` — detects systemd-resolved
16. `proxy_from_environment` — parses `http_proxy=` lines
17. `proxy_redaction_hint_for_credentials` — proxy URL with `user:pass@host` emits RedactionHint
18. `dnf_proxy` — parses proxy from dnf.conf
19. `is_proxy_line_true_cases` — matches `http_proxy=`, `HTTPS_PROXY=`, `no_proxy=`
20. `is_proxy_line_false_cases` — rejects non-proxy lines

- [ ] **Step 4: Verify**

Run: `cargo test --workspace`
Expected: All existing tests pass. New network tests pass.

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/inspectors/network.rs inspectah-collect/src/inspectors/mod.rs
git commit -m "feat(inspect): implement network inspector with NM/firewalld/routing/proxy collection"
```

---

## Task 3: Containers Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/containers.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs` (if not already done in Task 2)
- Modify: `inspectah-collect/Cargo.toml` (add `regex` to `inspectah-collect` only, not workspace-wide)

**Reference:** Go `cmd/inspectah/internal/inspector/container.go`

The containers inspector collects Quadlet unit files, docker-compose/compose YAML files, running containers (via podman), and installed Flatpak applications.

### Compose env contract (DECIDED)

`ComposeFile` stores `path` + `images` only — no `content` field, no `env` field. The inspector emits `RedactionHint`s for suspicious env var names found during compose file scanning (at inspector time, not engine time). No engine-level redaction proof for compose content is needed because nothing is persisted.

The schema is explicitly additive-ready: a future `Option<Vec<ComposeEnvVar>>` field with `#[serde(default)]` can be added later as a non-breaking change. This plan does NOT add that field.

### Go function parity map

| Go function | Rust equivalent | Notes |
|-------------|-----------------|-------|
| `RunContainers` | `ContainersInspector::inspect()` | Orchestrates all sub-collectors |
| `scanQuadletDir` | `scan_quadlet_dir()` | Walk dir for .container/.volume/.network/.kube/.pod/.image/.build files |
| `extractQuadletImage` | `extract_quadlet_image()` | Parse `Image=` from .container content |
| `ExtractQuadletPortsAndVolumes` | `extract_quadlet_ports_and_volumes()` | Parse `PublishPort=` and `Volume=` directives |
| `userQuadletDirs` | `user_quadlet_dirs()` | Discover per-user `~/.config/containers/systemd/` dirs |
| `findComposeFiles` | `find_compose_files()` | Walk /opt, /srv, /etc for compose YAML files |
| `extractComposeImages` | `extract_compose_images()` | Line-by-line YAML key extraction (no library) |
| `filteredWalk` | `filtered_walk()` | Recursive walk with prune markers and skip dirs |
| `queryPodmanContainers` | `query_podman_containers()` | `podman ps --format json` + `podman inspect` |
| `parsePodmanInspect` | `parse_podman_inspect()` | Extract ID, name, image, status, mounts, networks, ports, env, restart policy |
| `parsePodmanPS` | `parse_podman_ps()` | Fallback when podman inspect unavailable |
| `detectFlatpakApps` | `detect_flatpak_apps()` | `flatpak list --app --system` + `flatpak remote-list` |
| `parseMounts` | `parse_mounts()` | Extract mount type/source/dest/mode/rw from inspect data |
| `parseNetworking` | `parse_networking()` | Extract networks and ports from NetworkSettings |
| `extractRestartPolicy` | `extract_restart_policy()` | Extract from HostConfig.RestartPolicy.Name |

### Key constants (match Go)

```rust
const QUADLET_EXTENSIONS: &[&str] = &[
    ".container", ".volume", ".network", ".kube", ".pod", ".image", ".build",
];
const COMPOSE_PATTERNS: &[&str] = &[
    "docker-compose*.yml", "docker-compose*.yaml",
    "compose*.yml", "compose*.yaml",
];
const COMPOSE_SEARCH_DIRS: &[&str] = &["opt", "srv", "etc"];
const NON_SYSTEM_UID_MIN: u32 = 1000;
const NON_SYSTEM_UID_MAX: u32 = 60000;
```

### Degraded handling

- Quadlet system dir (`/etc/containers/systemd/`, `/usr/share/containers/systemd/`) PermissionDenied → Degraded
- Quadlet system dir NotFound → silent skip
- Compose file parse error (malformed YAML) → skip file, Degraded (not panic or silent skip). Explicit test required.
- `podman ps` failure (exit code != 0) → warning, skip live container data
- `podman inspect` failure → fall back to `parsePodmanPS` (basic fields only)
- JSON parse error on podman output → Degraded
- `flatpak` not installed (`which flatpak` fails) → skip Flatpak section

### Redaction surfaces

- Compose file env vars: emit `RedactionHint` at inspector time for environment blocks containing secret-like names (`PASSWORD`, `SECRET`, `TOKEN`, `KEY`, `CREDENTIAL`). These hints flow into the secrets-review artifact. No engine-level compose redaction — nothing is persisted.
- Podman inspect env: `running_containers[].env[]` entries are persisted. Engine-level scan via `redact_string()` catches secret patterns.

- [ ] **Step 1: Register module**

Add `pub mod containers;` to `inspectah-collect/src/inspectors/mod.rs` (if not already added).

- [ ] **Step 2: Add `regex` dependency**

Add `regex = "1"` to `[dependencies]` in `inspectah-collect/Cargo.toml` only (NOT in root workspace Cargo.toml). Needed for compose image extraction pattern matching, matching Go's `regexp.MustCompile`.

- [ ] **Step 3: Implement `ContainersInspector`**

Create `inspectah-collect/src/inspectors/containers.rs`:

```rust
pub struct ContainersInspector;

impl Inspector for ContainersInspector {
    fn id(&self) -> InspectorId { InspectorId::Containers }
    fn applicable_to(&self) -> &[SourceSystemKind] { &[SourceSystemKind::PackageBased] }
    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        // ... orchestrate: quadlets → compose → podman → flatpak
    }
}
```

Key implementation details:
- Quadlet scanning: walk system dirs (`/etc/containers/systemd/`, `/usr/share/containers/systemd/`) and per-user dirs
- Per-user quadlet: read `/etc/passwd` to find non-system users (UID 1000–60000), check `~/.config/containers/systemd/`
- Compose image extraction: line-by-line scanner matching Go's `extractComposeImages` — detect `services:` block, calibrate indent, find `image:` keys per service
- Compose env hint: while scanning compose content, if `environment:` block found with secret-pattern names → emit `RedactionHint`. Do NOT persist the env content.
- Podman: run `podman ps --format json`, then `podman inspect` for each container ID
- Podman inspect parsing: `serde_json::Value` dynamic extraction of ID, Name, Image, ImageID, Status, RestartPolicy, Mounts, Networks, Ports, Env
- Flatpak: `which flatpak` → `flatpak list --app --system --columns=application,origin,branch` → `flatpak remote-list --system --columns=name,url`
- `filteredWalk`: shared with services inspector for dev-artifact pruning (prune markers, skip dirs)

- [ ] **Step 4: Write unit tests (19–27 in-module tests)**

Unit tests in `#[cfg(test)] mod tests`:

1. `quadlet_container_unit` — parse Image, PublishPort, Volume from .container
2. `quadlet_volume_unit` — .volume file recognized
3. `quadlet_network_unit` — .network file recognized
4. `quadlet_all_extensions` — all 7 extensions recognized
5. `quadlet_image_extraction` — various `Image=` formats
6. `quadlet_system_dir_not_found` — silent skip
7. `quadlet_system_dir_permission_denied` — Degraded
8. `user_quadlet_dirs` — discovers per-user dirs from passwd
9. `compose_image_extraction_2space` — 2-space indent YAML
10. `compose_image_extraction_4space` — 4-space indent YAML
11. `compose_image_extraction_tab` — tab-indented YAML
12. `compose_no_services_block` — YAML without `services:` → empty
13. `compose_malformed_yaml_degraded` — broken YAML → Degraded status, not panic or silent skip
14. `compose_valid_but_unsupported_yaml_degraded` — valid YAML with features the lightweight line scanner cannot handle (e.g., YAML anchors/aliases: `image: *default_image` where `default_image` is defined as `&default_image registry.example.com/app:v1`) → Degraded status, not panic. The line scanner cannot resolve anchors; it must degrade gracefully rather than silently producing wrong output (e.g., storing `*default_image` as a literal image name) or panicking.
15. `compose_file_discovery` — finds compose files in /opt, /srv, /etc
16. `compose_env_secret_redaction_hint` — env with PASSWORD produces RedactionHint at inspector time
17. `podman_ps_and_inspect` — full pipeline: ps → inspect → RunningContainer
18. `podman_ps_only_fallback` — inspect fails → basic fields from ps
19. `podman_ps_failure` — exit code != 0 → warning, no containers
20. `podman_inspect_mounts` — mount parsing (type, source, dest, mode, rw)
21. `podman_inspect_restart_policy` — restart policy extraction
22. `podman_json_parse_error` — malformed JSON → Degraded
23. `podman_env_secret_redaction_hint` — container Env with TOKEN produces RedactionHint
24. `flatpak_apps_detected` — 3 apps parsed from flatpak list
25. `flatpak_not_installed` — `which flatpak` fails → empty
26. `flatpak_remotes` — remote-list parsed, Remote field populated
27. `empty_system_no_containers` — all empty → empty section, not degraded

- [ ] **Step 5: Verify**

Run: `cargo test --workspace`
Expected: All existing + network + container tests pass.

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 6: Commit**

```bash
git add inspectah-collect/src/inspectors/containers.rs inspectah-collect/src/inspectors/mod.rs
git add inspectah-collect/Cargo.toml
git commit -m "feat(inspect): implement containers inspector with quadlet/compose/podman/flatpak collection"
```

---

## Task 4: Users/Groups Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/users.rs`
- Modify: `inspectah-collect/src/inspectors/mod.rs` (if not already done)

**Reference:** Go `cmd/inspectah/internal/inspector/users.go`

The users/groups inspector collects non-system users and groups, sudoers rules, SSH key references, and sub{uid,gid} mappings. It classifies users using the two-strategy auto-detect model (sysusers/blueprint based on login shell) and supports operator overrides to `useradd` or `kickstart`.

### Go function parity map

| Go function | Rust equivalent | Notes |
|-------------|-----------------|-------|
| `RunUsersGroups` | `UsersGroupsInspector::inspect()` | Orchestrates all parsing |
| `parsePasswd` | `parse_passwd()` | Filter non-system users (UID 1000–60000) |
| `parseShadow` | `parse_shadow()` | Extract expiry/status only — **NEVER store hashes** |
| `parseGroup` | `parse_group()` | Filter non-system groups (GID 1000–60000) |
| `parseGshadow` | `parse_gshadow()` | Extract admin/member lists only — **NEVER store password/hash field** |
| `parseSubIDFile` | `parse_subid_file()` | Parse `/etc/subuid` and `/etc/subgid` |
| `parseSudoers` | `parse_sudoers()` | Read `/etc/sudoers` + `/etc/sudoers.d/*` |
| `extractSudoersRules` | `extract_sudoers_rules()` | Filter comment/blank lines, include directives |
| `collectSSHKeys` | `collect_ssh_keys()` | Count keys in `~/.ssh/authorized_keys` per user — **presence/count only, not key content** |
| `classifyUser` | `classify_user()` | **Deliberate divergence:** Go uses shell+home+UID for three-way (service/human/ambiguous). Rust uses shell-only for two-way (sysusers/blueprint). See `testdata/divergences.md`. |
| `assignGroupStrategies` | `assign_group_strategies()` | Map classification → strategy. Auto-detect: sysusers/blueprint only. Override: useradd/kickstart via internal inspector option (CLI flag deferred). |

### Key constants (match Go)

```rust
const NON_SYSTEM_UID_MIN: u32 = 1000;
const NON_SYSTEM_UID_MAX: u32 = 60000; // exclusive
const NON_SYSTEM_GID_MIN: u32 = 1000;
const NON_SYSTEM_GID_MAX: u32 = 60000; // exclusive

const NOLOGIN_SHELLS: &[&str] = &["/sbin/nologin", "/bin/false", "/usr/sbin/nologin"];
const VALID_LOGIN_SHELLS: &[&str] = &[
    "/bin/bash", "/bin/zsh", "/bin/sh", "/bin/fish", "/bin/tcsh", "/bin/csh",
    "/usr/bin/bash", "/usr/bin/zsh", "/usr/bin/fish",
];

// Two-strategy auto-detect model (deliberate divergence from Go):
// valid login shell   → "blueprint"  (image-builder provisioning — FIXME/deferred in this slice)
// no valid login shell → "sysusers"  (sysusers.d drop-in — FIXME/deferred in this slice)
//
// Override-only strategies (never auto-assigned):
// "useradd"    — set via internal inspector option → RUN useradd in Containerfile
// "kickstart"  — set via internal inspector option → user command in kickstart.ks
// CLI flag --user-strategy-override is deferred (not budgeted in Slice 2b).
```

### Sensitive input handling (CRITICAL)

**`/etc/shadow` non-persistence contract:**

Parse fields by `:` delimiter. The `shadow_entries: Vec<String>` field in `UserGroupSection` stores **stripped lines only**: `username:STATUS:lastchg:min:max:warn:inactive:expire:reserved` where STATUS is one of `locked`, `disabled`, `password_set`, `no_password` — NEVER the actual hash value.

- Field 0: username (for matching to passwd entry)
- Field 1 (password hash): Read ONLY the first characters to determine status. `!!` = locked, `*` = disabled, empty = no password, `$` prefix = has hash. Record status string only. **The hash value NEVER enters any field of `UserGroupSection`, `InspectionSnapshot`, or rendered artifacts.**
- Fields 2–8: expiry fields (last change, min, max, warn, inactive, expire, reserved)

The stored entry format is: `username:STATUS:field2:field3:field4:field5:field6:field7:field8`

**`/etc/gshadow` non-persistence contract:**

Same treatment as shadow. The `gshadow_entries: Vec<String>` field stores: `groupname:!:admins:members` — the password/hash field (field 1) is always replaced with `!` regardless of actual content.

**Negative tests required:**
- `shadow_no_hash_in_json` — serialize entire snapshot to JSON, assert no `$6$`, `$y$`, `$5$`, `$2b$` prefix appears anywhere
- `gshadow_no_hash_in_json` — same for gshadow entries

**SSH keys:** Count lines (non-empty, non-comment). Store count and path reference. **Never store key content.**

**Sudoers:** Store rules. Emit `RedactionHint` for lines containing embedded passwords or tokens.

### Two-strategy auto-detect provisioning model

The inspector auto-assigns one of two strategies based solely on login shell:

1. **`sysusers`** (auto-detect) — no valid login shell. Artifact: sysusers.d drop-in file. **This slice assigns the strategy and emits a FIXME comment noting deferred sysusers.d materialization.** Test asserts: user has `strategy: "sysusers"`, renderer output contains FIXME comment.
2. **`blueprint`** (auto-detect) — valid login shell. Artifact: image-builder provisioning. **This slice assigns the strategy and emits a FIXME comment noting deferred carry-forward.** Test asserts: user has `strategy: "blueprint"`, renderer output contains FIXME comment.

**Override-only strategies** (never auto-assigned — accepted as internal inspector option; CLI flag `--user-strategy-override` deferred):

3. **`useradd`** (override) — Artifact: `RUN useradd` in Containerfile. Renderer handles this when the strategy is present in the snapshot. Unit tests prove the code path works via the internal inspector option. Test asserts: overridden user has `strategy: "useradd"`, containerfile output contains `RUN useradd <username>`.
4. **`kickstart`** (override) — Artifact: `user` command in kickstart-suggestion.ks. Renderer handles this when the strategy is present in the snapshot. Unit tests prove the code path works via the internal inspector option. Test asserts: overridden user has `strategy: "kickstart"`, kickstart output contains user entry.

Group strategy assignment:
- Each group's strategy follows the primary user's strategy (same as Go's `assignGroupStrategies()` behavior)
- When no primary user exists for a group, default to `sysusers`
- Override mode: if a strategy is explicitly set on a user/group via the internal inspector option, it takes precedence

### Degraded handling

- `/etc/passwd` read failure → InspectorError (fatal — no users to inspect)
- `/etc/shadow` PermissionDenied → Degraded (common for non-root)
- `/etc/shadow` NotFound → silent skip (unusual but valid)
- `/etc/group` read failure → Degraded (proceed with users only)
- `/etc/gshadow` failure → silent skip
- `/etc/sudoers` NotFound → no sudoers rules, not degraded
- `~/.ssh/` not accessible → skip that user's keys

- [ ] **Step 1: Register module**

Add `pub mod users;` to `inspectah-collect/src/inspectors/mod.rs`.

- [ ] **Step 2: Implement `UsersGroupsInspector`**

Create `inspectah-collect/src/inspectors/users.rs`:

```rust
pub struct UsersGroupsInspector;

impl Inspector for UsersGroupsInspector {
    fn id(&self) -> InspectorId { InspectorId::UsersGroups }
    fn applicable_to(&self) -> &[SourceSystemKind] { &[SourceSystemKind::PackageBased] }
    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        // ... orchestrate: passwd → shadow → group → gshadow → subid → sudoers → ssh → classify → assign strategies
    }
}
```

Key implementation details:
- Build `HashMap<String, bool>` of non-system usernames from passwd (UID 1000–60000)
- Build user entries as `serde_json::Value` objects with name, uid, gid, gecos, home, shell, classification, strategy
- Shadow: parse colon-delimited, extract expiry fields only. Replace hash field with status string. Store `username:STATUS:fields...` in `shadow_entries`. Add `password_status` (locked/disabled/set/none) to user entry — NOT the hash
- Gshadow: replace password/hash field with `!`, store `group:!:admins:members`. Merge admin/member lists into group entries
- Groups: filter non-system groups (GID 1000–60000), build as `serde_json::Value`
- SubID: filter to non-system users, store raw entries
- Sudoers: read `/etc/sudoers`, follow `#includedir` / `@includedir` to `/etc/sudoers.d/*`, extract non-comment/non-blank rules
- SSH keys: for each non-system user, check `<home>/.ssh/authorized_keys`, count keys, store `{user, key_count, path}` — NOT key content
- Classification: run `classify_user()` on each user entry — two-way shell check (valid login shell → `blueprint`, otherwise → `sysusers`)
- Strategy assignment: run `assign_group_strategies()` implementing the two-strategy auto-detect model. Override support: if the internal inspector option sets a user's strategy to `useradd` or `kickstart`, that takes precedence over auto-detection. (CLI flag `--user-strategy-override` is deferred.)
- Emit `RedactionHint` for sudoers lines matching secret patterns

- [ ] **Step 3: Write unit tests (20–25 in-module tests)**

Unit tests in `#[cfg(test)] mod tests`:

1. `parse_passwd_non_system_users` — UID 1000+ extracted, system users filtered
2. `parse_passwd_boundary_uids` — UID 999 excluded, 1000 included, 60000 excluded
3. `classify_user_valid_shell_blueprint` — user with `/bin/bash` → auto-detect `blueprint`
4. `classify_user_nologin_sysusers` — user with `/sbin/nologin` → auto-detect `sysusers`
5. `classify_user_unknown_shell_sysusers` — user with unknown shell (e.g., `/usr/local/bin/custom`) → auto-detect `sysusers`
6. `classify_user_bin_false_sysusers` — user with `/bin/false` → auto-detect `sysusers`
7. `classify_user_zsh_blueprint` — user with `/bin/zsh` → auto-detect `blueprint`
8. `classify_user_fish_blueprint` — user with `/usr/bin/fish` → auto-detect `blueprint`
9. `strategy_override_useradd` — user with internal override option set → strategy `useradd` (regardless of shell). Tests inspector-internal option, not CLI reachability.
10. `strategy_override_kickstart` — user with internal override option set → strategy `kickstart` (regardless of shell). Tests inspector-internal option, not CLI reachability.
10. `group_strategy_follows_primary_user` — group's strategy matches its primary user's strategy
11. `group_strategy_default_sysusers` — group with no primary user → default `sysusers`
12. `group_strategy_override` — explicit strategy overrides primary-user derivation
13. `shadow_expiry_extraction` — expiry fields parsed correctly, status string replaces hash
14. `shadow_locked_account` — `!!` prefix → `locked` status in stored entry
15. `shadow_disabled_account` — `*` → `disabled` status in stored entry
16. `shadow_no_hash_stored` — verify no `$6$`/`$y$`/`$5$`/`$2b$` pattern appears in stored shadow entry
17. `shadow_permission_denied_degraded` — PermissionDenied → Degraded but continues
18. `shadow_not_found_silent_skip` — NotFound → no shadow data, not degraded
19. `gshadow_strips_password_field` — password field replaced with `!` in stored entry
20. `gshadow_no_hash_in_stored_entry` — verify hash content never appears
21. `group_non_system_groups` — GID 1000+ extracted
22. `gshadow_merges_members` — admin/member lists merged into group entry
23. `subuid_subgid_parsing` — entries filtered to non-system users
24. `sudoers_rules_extracted` — rules parsed, comments/blanks filtered
25. `sudoers_includedir_followed` — `#includedir /etc/sudoers.d` → reads drop-ins
26. `sudoers_redaction_hint_for_password` — rule with embedded password emits hint
27. `ssh_key_count_not_content` — authorized_keys returns count, NOT key material
28. `ssh_dir_inaccessible` — skip user, not degraded
29. `passwd_read_failure_fatal` — `/etc/passwd` error → InspectorError
30. `empty_system_no_users` — no non-system users → empty section, not degraded

- [ ] **Step 4: Verify**

Run: `cargo test --workspace`
Expected: All existing + network + containers + users tests pass.

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/inspectors/users.rs inspectah-collect/src/inspectors/mod.rs
git commit -m "feat(inspect): implement users/groups inspector with two-strategy auto-detect, shadow safety, SSH key counting"
```

---

## Task 5: Integration Tests (Inspector-on-Fixture Proof Lane)

**Files:**
- Create: `inspectah-collect/tests/network_test.rs`
- Create: `inspectah-collect/tests/containers_test.rs`
- Create: `inspectah-collect/tests/users_test.rs`

These are the inspector-on-fixture proof lane tests, matching the pattern in `inspectah-collect/tests/parity_test.rs` for Slice 2a inspectors. They run the actual Rust inspectors on fixture data via MockExecutor and verify output is structurally correct.

- [ ] **Step 1: Create `inspectah-collect/tests/network_test.rs`**

Following the `parity_test.rs` pattern:
- Load network fixtures via `include_str!`
- Build MockExecutor with network command/file mocks
- Run `NetworkInspector::inspect()` on mock context
- Verify structural correctness (connections, firewall zones, routes, proxy entries)
- Verify JSON roundtrip through `NetworkSection`

Tests:
1. `test_network_inspector_happy_path` — all sub-collectors produce data
2. `test_network_inspector_nm_not_found` — no NM dir → still succeeds with empty connections
3. `test_network_inspector_degraded_permissions` — PermissionDenied → Degraded output
4. `test_network_inspector_json_roundtrip` — output round-trips through NetworkSection type

- [ ] **Step 2: Create `inspectah-collect/tests/containers_test.rs`**

1. `test_containers_inspector_happy_path` — quadlets + compose + podman + flatpak all produce data
2. `test_containers_inspector_empty_system` — all dirs missing → empty section
3. `test_containers_inspector_degraded_podman` — podman failure → Degraded
4. `test_containers_inspector_json_roundtrip` — output round-trips through ContainerSection type

- [ ] **Step 3: Create `inspectah-collect/tests/users_test.rs`**

1. `test_users_inspector_happy_path` — full population with both auto-detect strategies represented (sysusers + blueprint)
2. `test_users_inspector_shadow_strips_hashes` — verify shadow entries in output contain no hash patterns
3. `test_users_inspector_gshadow_strips_passwords` — verify gshadow entries contain no hash patterns
4. `test_users_inspector_two_strategy_autodetect` — verify shell-based classification: valid login shell → blueprint, nologin/unknown → sysusers
5. `test_users_inspector_group_strategy_follows_user` — verify group strategy derivation
6. `test_users_inspector_degraded_shadow` — PermissionDenied on shadow → Degraded
7. `test_users_inspector_json_roundtrip` — output round-trips through UserGroupSection type

- [ ] **Step 4: Verify**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/tests/network_test.rs
git add inspectah-collect/tests/containers_test.rs
git add inspectah-collect/tests/users_test.rs
git commit -m "test(collect): add inspector-on-fixture integration tests for network, containers, users"
```

---

## Task 6: Redaction Engine Extension

**Files:**
- Modify: `inspectah-pipeline/src/redaction/engine.rs`
- Create: `inspectah-pipeline/tests/redaction_2b_surfaces_test.rs`

Extend the redaction engine to scan the new persisted surfaces introduced by the Slice 2b inspectors. Compose env is NOT a persisted surface (per the hints-only decision) and is excluded from engine-level scanning.

### New surfaces to scan

| Surface | Source | Redaction behavior |
|---------|--------|-------------------|
| Proxy URLs | `NetworkSection.proxy[].line` | Dedicated `mask_proxy_credentials()` function (~30-40 lines in `engine.rs`) uses capture-group-aware replacement to mask only the password segment inline: `http://admin:[REDACTED]@proxy.corp.com:8080`. The generic `redact_string()` pass does NOT process proxy lines — the dedicated masker is the sole owner. This prevents the generic whole-match replacement from destroying the preserved-inline URL structure. |
| Podman container env | `ContainerSection.running_containers[].env[]` | Scan each env entry for `SECRET_PATTERNS` via `redact_string()` |
| Shadow data | `UserGroupSection.shadow_entries[]` | Already handled by `scan_shadow()` in patterns.rs — verify wiring to users inspector output |
| Sudoers rules | `UserGroupSection.sudoers_rules[]` | Scan for `SECRET_PATTERNS` in rule text via `redact_string()` |

**Not scanned by engine:** Compose env vars (nothing persisted to scan — hints emitted at inspector time).

### Planted-secret proof tests

Each test plants a known secret in inspector output and verifies the redaction engine detects it.

- [ ] **Step 1a: Add `mask_proxy_credentials()` to engine.rs and extend `redact()`**

Add a dedicated `mask_proxy_credentials()` function (~30-40 lines) in `inspectah-pipeline/src/redaction/engine.rs`. This function is the **sole owner** of proxy line masking — the generic `redact_string()` pass does not process `network.proxy[].line` fields.

This separation is necessary because `redact_string()` does whole-match replacement (`find_iter()` + `token_for()`) and cannot produce `http://user:[REDACTED]@host` — it would replace the entire `://user:password@` match, destroying the URL structure.

The `mask_proxy_credentials()` function must:
1. Use a regex with a capture group: `r"(://[^:/@\s]+:)([^@\s]+)(@)"` — group 1 is `://user:`, group 2 is the password, group 3 is `@`
2. Replace ONLY group 2 with `[REDACTED]`, preserving the rest of the URL structure
3. Generate a `RedactionFinding` (kind: `Password`, confidence: `High`, detection: `Pattern`) when a match is found
4. Return `Cow<'_, str>` (borrowed if no match, owned if masked) — matching `redact_string()` convention

The engine's `redact()` function calls `mask_proxy_credentials()` on each `network.proxy[].line` value. These fields are **excluded** from the generic `redact_string()` pass to prevent the whole-match replacer from re-matching and destroying the preserved inline URL shape.

Add scanning passes for:
- `snapshot.containers.running_containers` → `redact_string()` on each `.env[]` entry
- `snapshot.users_groups.sudoers_rules` → `redact_string()` on each rule
- `snapshot.users_groups.shadow_entries` → `scan_shadow()` (already exists, verify wiring)

The existing `RedactionHint` pipeline handles inspector-time hints (including compose env hints). This task ensures the engine's post-collection pass also catches secrets in the persisted data.

- [ ] **Step 2: Write planted-secret proof tests**

Create `inspectah-pipeline/tests/redaction_2b_surfaces_test.rs`:

1. `proxy_url_with_password_masked_inline` — proxy line `http_proxy=http://admin:secret123@proxy.corp.com:8080` → after `mask_proxy_credentials()`, password masked to `http_proxy=http://admin:[REDACTED]@proxy.corp.com:8080`, finding detected. Verify the full URL structure is preserved (scheme, user, host, port) with only the password segment replaced.
2. `podman_env_with_secret_redacted` — container env `DB_PASSWORD=hunter2` → finding detected
3. `sudoers_with_embedded_password_redacted` — sudoers rule containing password → finding detected
4. `shadow_hash_detected` — shadow entry with `$6$...` hash → finding detected
5. `shadow_locked_no_finding` — `!!` prefix → no finding (not a secret)
6. `clean_proxy_no_finding` — `http_proxy=http://proxy:8080` (no credentials) → no finding
7. `clean_env_no_finding` — container env `PORT=8080` → no finding

**Removed:** `compose_env_with_token_redacted` — compose env is not persisted, so there is no engine-level surface to test. Inspector-time RedactionHint for compose env is tested in Task 3 unit tests.

- [ ] **Step 3: Verify**

Run: `cargo test --workspace`
Expected: All tests pass including new redaction tests.

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/src/redaction/engine.rs
git add inspectah-pipeline/tests/redaction_2b_surfaces_test.rs
git commit -m "feat(redaction): add proxy credential masker, extend engine to scan container env, sudoers surfaces"
```

---

## Task 7: Parity Gate Expansion (Serde/Golden Proof Lane)

**Files:**
- Create: `testdata/golden/go-v13-network-section.json` (provisional)
- Create: `testdata/golden/go-v13-containers-section.json` (provisional)
- Create: `testdata/golden/go-v13-users-groups-section.json` (provisional)
- Modify: `inspectah-core/tests/parity_gate.rs`
- Modify: `testdata/divergences.md`

### Golden file generation

Golden files are generated by running Go inspectah on a real RHEL system:

```bash
inspectah scan --output /tmp/go-scan
jq '.network' /tmp/go-scan/inspection-snapshot.json > go-v13-network-section.json
jq '.containers' /tmp/go-scan/inspection-snapshot.json > go-v13-containers-section.json
jq '.users_groups' /tmp/go-scan/inspection-snapshot.json > go-v13-users-groups-section.json
```

**Provisional goldens from Go test fixtures are acceptable for CI during development, but do NOT satisfy slice-closure evidence.** Before slice sign-off (Task 10), provisional goldens must be replaced with real scan output and the host-validation evidence must reference the same host.

- [ ] **Step 1: Generate golden files** (provisional or real)

- [ ] **Step 2: Expand serde/golden roundtrip tests**

Add per-section serde roundtrip tests to `inspectah-core/tests/parity_gate.rs`:

1. `test_network_serde_roundtrip` — Go golden → `NetworkSection` → JSON → diff through allowlist
2. `test_network_field_coverage` — verify structural fields (connections, firewall_zones, proxy, etc.)
3. `test_containers_serde_roundtrip` — Go golden → `ContainerSection` → JSON → diff
4. `test_containers_field_coverage` — verify quadlet_units, compose_files, running_containers, flatpak
5. `test_users_groups_serde_roundtrip` — Go golden → `UserGroupSection` → JSON → diff
6. `test_users_groups_field_coverage` — verify users, groups, sudoers_rules, shadow_entries

These tests prove type-level compatibility. They do NOT exercise Rust inspector code — that is the inspector-on-fixture lane (Task 5).

- [ ] **Step 3: Document any divergences**

Add entries to `testdata/divergences.md` using the governed format:

```markdown
### Section Name

#### divergence title
- Go: [what Go produces]
- Rust: [what Rust produces]
- Reason: [why different]
- Disposition: [permanent | temporary | bug]
- Approved: [pending | date]
```

Expected divergences:
- **users provisioning strategy model**: Go uses three-way classification (service/human/ambiguous) mapping to sysusers/kickstart/useradd with blueprint as an override. Rust uses two-way auto-detect (valid login shell → blueprint, no valid login shell → sysusers) with useradd/kickstart as override-only. This is a deliberate product decision — not a parity bug. Disposition: permanent.
- **users classification ordering**: Go and Rust may produce users in different iteration order — normalize by sorting before comparison
- **proxy line formatting**: minor whitespace differences in proxy entries

- [ ] **Step 4: Commit**

```bash
git add testdata/golden/ inspectah-core/tests/parity_gate.rs testdata/divergences.md
git commit -m "test(parity): expand serde/golden roundtrip gate to network, containers, users_groups sections"
```

---

## Task 8: Renderer Smoke Tests

**Files:**
- Create: `inspectah-pipeline/tests/smoke_render_2b.rs`
- Modify (if needed): renderer source files to handle new section sub-fields correctly

Verify that the **existing** renderers (containerfile, configtree, kickstart, readme) produce correct output for the 3 new sections. Audit (`audit.rs`) and report (`report.rs`) rendering for these sections is **deferred** to a later slice — this task does NOT test or budget those consumers.

These are integration tests that build a snapshot with populated section data and verify the rendered output contains expected content.

- [ ] **Step 1: Write renderer smoke tests**

Create `inspectah-pipeline/tests/smoke_render_2b.rs`:

**Containerfile renderer tests:**
1. `containerfile_network_firewall_copy_comment` — network section with firewall zones → Containerfile contains zone count + COPY comment (NOT `firewall-cmd` directives — the current renderer uses declarative config-copy handling)
2. `containerfile_network_static_routes` — static routes → comment lines referencing route files
3. `containerfile_network_hosts_additions` — hosts additions → FIXME comments
4. `containerfile_network_proxy` — proxy entries → comment lines with source + line
5. `containerfile_containers_quadlet_copy` — containers section with included quadlet units → `COPY quadlet/ /etc/containers/systemd/` line
6. `containerfile_containers_no_compose_comments` — containers section with compose files → NO compose-image comments in Containerfile (compose/running-container rendering is deferred — informational in snapshot only)
7. `containerfile_users_useradd_override` — users section with a snapshot where users have override-assigned `useradd` strategy (set via internal inspector option, not CLI) → `RUN useradd` commands. Tests renderer behavior given override data, not CLI reachability.
8. `containerfile_users_sysusers_comment` — users section with auto-detect sysusers users → count + config-tree comment
9. `containerfile_users_blueprint_fixme` — users section with auto-detect blueprint users → FIXME comment noting image-builder provisioning

**Configtree renderer tests:**
10. `configtree_firewall_zones_materialized` — network section with included firewall zones → zone XML files written under `config/etc/firewalld/zones/`
11. `configtree_quadlet_units_materialized` — containers section with included quadlet units → unit files written under `quadlet/` (top-level output directory, NOT under `config/etc/containers/systemd/`)
12. `configtree_flatpak_manifest_and_service` — containers section with included flatpak apps → `flatpak/flatpak-install.json` manifest and `flatpak/flatpak-provision.service` written under output directory. Verify manifest contains app_id/remote/branch fields. Verify service file contains `flatpak install` commands.

**Containerfile flatpak test:**
13. `containerfile_containers_flatpak_copy_and_enable` — containers section with included flatpak apps → Containerfile contains `COPY flatpak/` line and `RUN systemctl enable flatpak-provision.service`. Flatpak is actively rendered by both containerfile and configtree renderers, not snapshot-only.

**Kickstart renderer tests:**
14. `kickstart_network_dhcp_connections` — DHCP connections → `network --bootproto=dhcp` lines
15. `kickstart_network_static_connections` — static connections → FIXME with `network --bootproto=static`
16. `kickstart_network_hosts_routes` — hosts additions and static routes → kickstart entries
17. `kickstart_users_override_kickstart` — snapshot with override-assigned `kickstart` strategy (set via internal inspector option, not CLI) → `user` commands. Tests renderer behavior given override data, not CLI reachability.

**Readme renderer tests:**
18. `readme_container_workload_summary` — containers section with quadlet units and compose files → findings summary table includes `"Container workloads | {q} quadlet, {c} compose"` row. Verify counts match the number of items in the snapshot, not image or running-container counts.

**Cross-cutting tests:**
19. `containerfile_empty_sections` — empty network/containers/users → no crash, no section output
20. `containerfile_degraded_sections` — degraded completeness → FIXME comments in Containerfile

Each test:
- Builds an `InspectionSnapshot` with the target section populated
- Calls the renderer
- Asserts output contains expected strings matching the ACTUAL renderer contract (per the artifact-consumer matrix above)
- Asserts no panics on empty/None sections

- [ ] **Step 2: Fix any renderer gaps**

If any existing renderer function is missing handling for a sub-field needed by these smoke tests, fix it inline in this task. Document what was changed. Expected: minimal to no renderer changes — the existing functions already consume these sections. If substantial renderer work surfaces, flag it as scope creep and file a separate task.

- [ ] **Step 3: Verify**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/tests/smoke_render_2b.rs
git commit -m "test(render): add renderer smoke tests for network, containers, users sections against existing consumers"
```

---

## Task 9: Failure Policy Tests

**Files:**
- Create: `inspectah-pipeline/tests/failure_policy_2b.rs`

Verify the degraded/failed semantics for the 3 new inspectors match the spec.

- [ ] **Step 1: Write failure policy tests**

Tests covering:

1. `network_permission_denied_degraded` — PermissionDenied on NM dir → Completeness::Partial with degraded_sections containing Network
2. `network_not_found_not_degraded` — NotFound on all network dirs → still Complete (nothing to inspect)
3. `containers_podman_failure_degraded` — podman ps fails → Partial
4. `containers_all_dirs_missing_complete` — no quadlet/compose/podman/flatpak → still Complete
5. `users_passwd_failure_incomplete` — /etc/passwd read failure → Incomplete (fatal)
6. `users_shadow_permission_denied_degraded` — shadow PermissionDenied → Partial
7. `mixed_failures_across_inspectors` — one inspector Degraded, another Failed → Incomplete

- [ ] **Step 2: Verify**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add inspectah-pipeline/tests/failure_policy_2b.rs
git commit -m "test(policy): add failure policy tests for network, containers, users inspectors"
```

---

## Task 10: Host Validation & Golden File Finalization

**Files:**
- Modify: `testdata/golden/go-v13-*-section.json` (replace provisional with real for all 7 sections)
- Modify: `scripts/host-validation.sh` (extend to cover all 7 sections)
- Create: `testdata/evidence/slice-2b-host-validation.md`

This task closes BOTH Slice 2a and Slice 2b host validation evidence in one pass. It covers all 7 sections (RPM + services + storage + kernelboot + network + containers + users_groups).

The task uses the **real Rust CLI contract**. Check `inspectah-cli/src/commands/scan.rs` for the actual flow:
- `--inspect-only` writes JSON snapshot directly
- Without `--inspect-only`, the full pipeline runs: detect → collect → validate → redact → render → tarball

- [ ] **Step 1: Extend `scripts/host-validation.sh`**

Update the host validation script to cover all 7 sections:
- Add section extraction for `network`, `containers`, `users_groups` (alongside existing `services`, `storage`, `kernel_boot`). Note: jq field names use underscores (`kernel_boot`, `users_groups`) but golden file stems use hyphens (`kernelboot`, `users-groups`).
- Add section-level diff for all 7 sections
- The script must use `inspectah-cli scan --inspect-only` for the Rust binary (matching the actual CLI)

- [ ] **Step 2: Run Go and Rust on same CentOS Stream 9 host**

```bash
# Go scan
inspectah scan --output /tmp/go-scan

# Rust scan (on rust branch, using real CLI)
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
cargo run -p inspectah-cli -- scan --inspect-only --output /tmp/rust-snapshot.json
```

- [ ] **Step 3: Generate and compare golden files for all 7 sections**

```bash
# Extract all sections — note the golden file naming convention uses
# hyphenated stems WITHOUT underscores (e.g., kernelboot not kernel_boot,
# users-groups not users_groups). Match the existing convention:
jq '.rpm' /tmp/go-scan/inspection-snapshot.json > testdata/golden/go-v13-rpm-section.json
jq '.services' /tmp/go-scan/inspection-snapshot.json > testdata/golden/go-v13-services-section.json
jq '.storage' /tmp/go-scan/inspection-snapshot.json > testdata/golden/go-v13-storage-section.json
jq '.kernel_boot' /tmp/go-scan/inspection-snapshot.json > testdata/golden/go-v13-kernelboot-section.json
jq '.network' /tmp/go-scan/inspection-snapshot.json > testdata/golden/go-v13-network-section.json
jq '.containers' /tmp/go-scan/inspection-snapshot.json > testdata/golden/go-v13-containers-section.json
jq '.users_groups' /tmp/go-scan/inspection-snapshot.json > testdata/golden/go-v13-users-groups-section.json
```

- [ ] **Step 4: Verify trust-bearing fields in raw Rust output**

Before normalization strips `redaction_state` and `completeness`, verify these fields are populated correctly in the raw Rust JSON:

```bash
# Check redaction_state is populated
jq '.redaction_state' /tmp/rust-snapshot.json

# Check completeness is populated
jq '.completeness' /tmp/rust-snapshot.json

# Both must be non-null and contain expected structure
```

This is required because the normalizer strips these fields for parity comparison, but they are trust-bearing and must be proven correct.

- [ ] **Step 5: Run section-level comparison**

```bash
# Run parity tests with real golden files.
# Note: `cargo test parity` matches ZERO tests (filter applies to function
# names, not binary names). Use --test to target the binary directly:
cargo test --test parity_gate -- --nocapture
cargo test --test parity_test -- --nocapture
```

All parity tests must pass with real golden files. Current test names for reference:
- `parity_gate.rs`: `test_snapshot_serde_roundtrip`, `test_{services,storage,kernelboot}_serde_roundtrip`, `test_{services,storage,kernelboot}_field_coverage`, `test_all_section_goldens_are_valid_json`
- `parity_test.rs`: `test_services_inspector_correctness`, `test_kernelboot_inspector_correctness`, `test_storage_inspector_vs_golden`

- [ ] **Step 6: Fill evidence artifact with real data**

Create `testdata/evidence/slice-2b-host-validation.md`:

```markdown
# Slice 2b Host Validation Evidence

**Date:** [actual date]
**Scope:** Closes both Slice 2a and Slice 2b host validation evidence

## Host Details
- **OS:** [actual, e.g., CentOS Stream 9]
- **Kernel:** [actual]
- **Architecture:** [actual]
- **Go inspectah version:** [actual]
- **Rust inspectah version:** [actual from Cargo.toml]

## Sections Validated (all 7)

### Slice 2a sections
- [x] rpm — [match / divergences noted]
- [x] services — [match / divergences noted]
- [x] storage — [match / divergences noted]
- [x] kernel_boot — [match / divergences noted]

### Slice 2b sections
- [x] network — [match / divergences noted]
- [x] containers — [match / divergences noted]
- [x] users_groups — [match / divergences noted]

## Trust-Bearing Fields (Rust-only)
- **redaction_state:** [populated / structure described]
- **completeness:** [populated / structure described]
- Note: These fields are stripped by normalize.rs for parity comparison
  but must be correct in the raw Rust output.

## Test Results
- Total tests: [actual count]
- Parity gate (serde/golden): [pass/fail]
- Inspector-on-fixture: [pass/fail]
- Clippy: [clean/warnings]

## Divergences
[List any divergences found, with references to testdata/divergences.md entries]
```

- [ ] **Step 7: Commit**

```bash
git add testdata/golden/ testdata/evidence/slice-2b-host-validation.md scripts/host-validation.sh
git commit -m "evidence(slice-2b): host validation on CentOS Stream 9 covering all 7 sections"
```

---

## Task 11: Final Verification

- [ ] **Step 1: Full test suite**

Run: `cargo test --workspace 2>&1 | grep 'test result'`
Record total test count. Target: Slice 2a baseline (~339) + Slice 2b additions (~74) = ~413+.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 3: Format check**

Run: `cargo fmt --all -- --check`
Expected: No issues.

- [ ] **Step 4: Verify slice checklist**

- [ ] Network, containers, users/groups inspectors implemented with borrowed `InspectionContext<'_>`
- [ ] All inspectors declare `fn applicable_to(&self) -> &[SourceSystemKind]` returning `&[SourceSystemKind::PackageBased]`
- [ ] All three slot into Wave 1 parallel execution (no orchestration changes)
- [ ] Three proof lanes populated for all 3 new sections:
  - [ ] Serde/golden roundtrip in `parity_gate.rs` (7 sections total)
  - [ ] Inspector-on-fixture in `inspectah-collect/tests/{network,containers,users}_test.rs`
  - [ ] Host validation evidence in `testdata/evidence/slice-2b-host-validation.md` (covering 2a + 2b)
- [ ] Renderer smoke tests passing for all 3 new sections against existing consumers only (containerfile, configtree, kickstart, readme)
- [ ] Failure policy tested: degraded/failed for all 3 inspectors
- [ ] Redaction engine extended for proxy, container env, sudoers surfaces with planted-secret proofs
- [ ] Sensitive input handling verified:
  - [ ] Shadow: stripped entries (no hashes). Negative JSON test proves no hash prefix in snapshot.
  - [ ] Gshadow: password field replaced with `!`. Negative JSON test proves no hash content.
  - [ ] SSH keys: count only
  - [ ] Sudoers: redaction scanned
  - [ ] Proxy: redaction scanned for embedded credentials
  - [ ] Compose env: inspector-time RedactionHints only (nothing persisted)
- [ ] Two-strategy auto-detect provisioning model: sysusers (FIXME, no valid login shell) and blueprint (FIXME, valid login shell). Override-only: useradd and kickstart (accepted as internal inspector option for unit test coverage; CLI flag `--user-strategy-override` deferred).
- [ ] Deliberate Go divergence documented in `testdata/divergences.md` (three-way classification → two-way shell check)
- [ ] Group strategy follows primary user. Default sysusers. Override respected.
- [ ] Trust-bearing fields (redaction_state, completeness) verified in raw Rust output before normalization
- [ ] Host validation evidence committed with real data (closes both 2a and 2b)
- [ ] No provisional golden files remaining
- [ ] All divergence allowlist entries have review-approval annotations
- [ ] All commits follow conventional commit format

- [ ] **Step 5: Review commit history**

Run: `git log --oneline` (Slice 2b commits)
Verify focused, well-described commits following conventional format.

---

## Execution Method

- SDD cadence: implementation agent works per task, review agent checkpoints after each
- Both agents use `claude-opus-4-6`
- Superpowers skill: `superpowers:subagent-driven-development`
- Cargo PATH: `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`

## Task Dependency Graph

```
Task 1 (fixtures) ──┬── Task 2 (network) ──┐
                    ├── Task 3 (containers) ┼── Task 5 (integration tests) ── Task 6 (redaction) ── Task 7 (parity gate) ── Task 8 (renderer smoke) ── Task 9 (failure policy) ── Task 10 (host validation) ── Task 11 (final)
                    └── Task 4 (users) ─────┘
```

Tasks 2, 3, 4 can run in parallel after Task 1 completes.
Tasks 5–11 are sequential.
