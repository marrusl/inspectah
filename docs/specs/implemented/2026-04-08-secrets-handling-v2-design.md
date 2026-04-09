# Secrets Handling v2: Detection Gaps, UX, and Remediation Clarity

**Date:** 2026-04-08 (revised after team review)
**Status:** Proposed
**Author:** Kiwi (orchestrator, synthesizing team input)
**Reviewers:** Kit (code audit), Slate (security assessment), Thorn (test coverage audit), Fern (UX), Ember (product strategy)
**Synthesis thread:** `comms/threads/2026-04-08-yoinkc-secrets-in-output-review.md`
**Supersedes:** v1 of this spec (pre-review draft)

---

## Overview

Close detection gaps in the primary pattern set, make redacted files obvious and actionable in the output, and give users clear remediation guidance based on the type of secret found.

**Scope boundary:** This spec covers detection and UX only. The independent heuristic safety net (entropy analysis, broad PEM detection, binary file detection) is deferred to a separate spec — it has different precision/recall tradeoffs and needs its own design.

## Context

### What exists today

The secrets pipeline has two stages:

1. **Primary detection** (`redact.py`): `redact_snapshot()` runs against the inspection snapshot before rendering. Two mechanisms:
   - `EXCLUDED_PATHS` (6 regex patterns): match file paths, replace content with a 54-byte placeholder (`# Content excluded (sensitive path). Handle manually.`). Currently: `/etc/shadow`, `/etc/gshadow`, `ssh_host_.*`, `/etc/pki/.*\.key`, `.*\.key$`, `.*keytab$`.
   - `REDACT_PATTERNS` (15 content patterns): match within file content, replace matched values with `REDACTED_{TYPE}_{hash}`. Cover PEM private keys, API keys, tokens, passwords, AWS/GitHub/GCP/Azure credentials, database connection strings.

2. **Safety net** (`scan_directory_for_secrets()`): runs post-render against files on disk before git push. Uses the same `REDACT_PATTERNS` list as primary detection. Deferred to a separate spec for redesign.

Redactions are logged to `snapshot.redactions[]` and rendered into `secrets-review.md` in the output.

### What's wrong

A real user ran yoinkc on a Kinoite machine. The output tarball contained cockpit self-signed cert files including a `.key` file. Investigation revealed:

- **The system worked**: the `.key` file content was correctly replaced with a placeholder. No actual key material leaked.
- **The UX is confusing**: the placeholder file still appears in the tarball with its original name, alarming users who see a `.key` file and assume their private key was included.
- **Detection gaps exist**: WireGuard private keys (bare base64, not PEM-wrapped), container registry auth files, WiFi PSKs, and binary keystores are not covered by any pattern.
- **Remediation guidance is generic**: all redacted items say "handle manually" regardless of whether the correct action is to regenerate on target, provision from a secret store, or supply a value at deploy time.
- **Hash tokens leak information**: inline redaction placeholders include a truncated SHA-256 of the original secret, creating a dictionary oracle for weak secrets like WiFi PSKs.

---

## Decisions Log

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Two-tier redaction: inline for mixed-content text files, full exclusion for opaque/binary secrets | Maps to user mental model — a WireGuard config "has settings plus a secret" while a `.p12` file "is the secret." Fern confirmed. |
| 2 | Excluded files written to `redacted/` directory with `.REDACTED` suffix | Outside the `config/` COPY tree — no risk of accidental inclusion in image. Fern's recommendation, placement resolved in review. |
| 3 | Excluded files get `include=False` — skipped by Containerfile COPY | Fail-loud on target is correct for a migration tool. Missing key = clear "file not found" error. Placeholder key = runtime mystery. Fern + Ember aligned. |
| 4 | Containerfile gets separate comment blocks for excluded vs inline-redacted | Different remediation paths — grouping them misleads users. Fern + Ember review feedback. |
| 5 | Three remediation states, used consistently across all output | Regenerate-on-target, provision-from-store, value-removed-inline. Each has distinct operator action. |
| 6 | Drop hash tokens — use sequential counters | Hash of secret value is a dictionary oracle for weak secrets. Sequential counters per type provide cross-file correlation without leaking information. Slate confirmed no legitimate use for hashes. |
| 7 | Safety net redesign deferred to separate spec | Different precision/recall tradeoff. Ember recommended slicing to reduce complexity. |
| 8 | auth.json scope: `/etc/containers/auth.json` only | Podman's system-wide auth path. No inspector scope expansion. |

---

## Remediation State Model

Three states, used consistently in `.REDACTED` placeholder content, `secrets-review.md`, Containerfile comments, and CLI output:

| State | Operator Action | Example |
|-------|----------------|---------|
| **Regenerate on target** | Do nothing — service auto-generates on first boot | cockpit ws-certs.d, SSH host keys |
| **Provision from secret store** | Deploy via secrets management after build | WireGuard private keys, TLS certs, registry auth, binary keystores |
| **Value removed inline** | File included in image with secret replaced — supply actual value at deploy time | WireGuard config (PrivateKey field), WiFi config (psk field), app configs with API tokens |

Each redaction record carries its remediation state. The state is assigned by pattern — excluded paths map to either "regenerate" or "provision" based on the pattern definition, inline redactions always map to "value removed inline."

### Pattern-to-state mapping

| Pattern | Tier | Remediation |
|---------|------|-------------|
| `/etc/cockpit/ws-certs\.d/.*` | Exclude | Regenerate on target |
| `ssh_host_.*` | Exclude | Regenerate on target |
| `.*\.key$`, `/etc/pki/.*\.key` | Exclude | Provision from secret store |
| `.*keytab$` | Exclude | Provision from secret store |
| `/etc/shadow`, `/etc/gshadow` | Exclude | Provision from secret store |
| `.*\.p12$`, `.*\.pfx$`, `.*\.jks$` | Exclude | Provision from secret store |
| `/etc/containers/auth\.json`| Exclude | Provision from secret store |
| All `REDACT_PATTERNS` (inline) | Inline | Value removed inline |

---

## Workstream 1: Detection Gaps

### New EXCLUDED_PATHS entries (full-file exclusion)

| Pattern | Remediation | Rationale |
|---------|-------------|-----------|
| `.*\.p12$` | Provision | PKCS#12 binary keystore — entirely secret, cannot inline-redact |
| `.*\.pfx$` | Provision | Same as `.p12`, Windows naming convention |
| `.*\.jks$` | Provision | Java KeyStore — binary, opaque |
| `/etc/cockpit/ws-certs\.d/.*` | Regenerate | Auto-generated, hostname-specific, regenerated on target. Including even the public cert is harmful (wrong hostname/validity). |
| `/etc/containers/auth\.json` | Provision | Podman system-wide registry credentials — base64-encoded auth tokens |

### New REDACT_PATTERNS entries (inline redaction)

| Pattern name | Regex target | Example match |
|-------------|-------------|---------------|
| `WIREGUARD_KEY` | `PrivateKey\s*=\s*[A-Za-z0-9+/]{43}=` | `PrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5A=` |
| `WIFI_PSK` | `psk\s*=\s*\S+` | `psk=mysecretpassword` |

### Redaction placeholder format change

**Current:** `REDACTED_{TYPE}_{truncated_sha256_hash}` (e.g., `REDACTED_PASSWORD_a3b2c1d4`)

**New:** `REDACTED_{TYPE}_{N}` where `N` is a sequential counter per type, assigned deterministically within a single yoinkc run (sorted by discovery order: file path, then line number).

Examples:
```
REDACTED_PASSWORD_1
REDACTED_PASSWORD_2
REDACTED_API_KEY_1
REDACTED_WIREGUARD_KEY_1
```

This preserves type visibility, cross-file correlation (same secret value gets same counter), and idempotency (same input → same output) while eliminating the dictionary oracle risk.

File-backed and non-file-backed findings share one counter space per type. Ordering is deterministic: file-backed findings first (sorted by path, then line number), then non-file-backed findings (sorted by source, then path/identifier). This ensures the same input always produces the same counter assignments regardless of processing order.

### Cockpit ws-certs.d behavior

Exclude the entire directory. Cockpit auto-generates self-signed certs on first boot. Carrying them to the target means:
- Wrong hostname on the cert
- Possibly expired validity
- Shared private key across source and target

The correct migration behavior is to let cockpit regenerate certs on the target. Remediation state: **regenerate on target**.

### Mixed PEM bundle rule

Outside the cockpit-specific directory exclusion, a general rule applies to any file containing PEM-encoded material:

- **File contains only private key(s):** existing `.*\.key$` exclusion handles this.
- **File contains cert + private key combined** (e.g., a PEM bundle): **inline-redact the private key block only.** The `PRIVATE_KEY` content pattern already matches `BEGIN ... PRIVATE KEY` blocks. The certificate portion is kept — it's public data and useful for understanding the TLS setup on the target. Remediation: "value removed inline."
- **File contains only certificate(s):** no redaction needed. Certificates are public.

This rule is already implied by the existing `REDACT_PATTERNS` behavior (private key content pattern fires regardless of what else is in the file), but stating it explicitly prevents implementation drift between "inline-redact the key" and "exclude the whole file" for mixed PEM bundles.

---

## Workstream 2: UX Improvements

### Typed redaction findings model

Replace the current flat `snapshot.redactions[]` list with a typed model. Each finding carries:

```python
@dataclass
class RedactionFinding:
    path: str              # Original filesystem path or synthetic identifier
    source: str            # "file" | "shadow" | "container-env" | "timer-cmd" | "diff"
    kind: str              # "excluded" or "inline"
    pattern: str           # Pattern name that matched
    remediation: str       # "regenerate" | "provision" | "value-removed"
    line: int | None       # Line number (inline only, file-backed only)
    replacement: str | None  # Replacement token (inline only, e.g. REDACTED_PASSWORD_1)
```

The `source` field distinguishes file-backed findings (which produce `.REDACTED` artifacts and Containerfile comments) from non-file-backed findings (shadow entries, running-container env vars, timer command fields, `:diff` views). Non-file-backed findings appear in `secrets-review.md` only — they do not produce `.REDACTED` files or Containerfile entries, since there is no file to exclude or reprovision.

This model drives all downstream output: `.REDACTED` file generation, `secrets-review.md`, Containerfile comments, and CLI summary. One source of truth, no drift.

### File-level behavior for excluded paths

**Current:** Content replaced with placeholder, file written to `config/` with original name, `include=True`.

**New:**
1. `redact_snapshot()` sets `include=False` on the config entry
2. `redact_snapshot()` creates a `RedactionFinding` with the appropriate remediation state
3. `write_config_tree()` skips entries with `include=False` in the normal config write loop
4. A new post-render step writes `.REDACTED` files to `redacted/` directory (top-level, outside `config/`), preserving the path structure:

```
redacted/
  etc/
    cockpit/
      ws-certs.d/
        0-self-signed.key.REDACTED
        0-self-signed.cert.REDACTED
        0-self-signed-ca.pem.REDACTED
```

### `.REDACTED` placeholder content

Content varies by remediation state:

**Regenerate on target:**
```
# REDACTED by yoinkc — auto-generated credential
# Original path: /etc/cockpit/ws-certs.d/0-self-signed.key
# Action: no action needed — this file is regenerated automatically on the target system
# See secrets-review.md for details
```

**Provision from secret store:**
```
# REDACTED by yoinkc — sensitive file detected
# Original path: /etc/pki/tls/private/server.key
# Action: provision this file on the target system from your secrets management process
# See secrets-review.md for details
```

### Containerfile comment blocks

Two separate blocks — one for excluded files, one for inline-redacted files:

```dockerfile
# === Excluded secrets (not in this image) ===
# These files were detected on the source system but excluded from the
# image. See redacted/ directory for details.
#
# Regenerate on target (auto-generated, no action needed):
#   /etc/cockpit/ws-certs.d/0-self-signed.key
#   /etc/cockpit/ws-certs.d/0-self-signed.cert
#
# Provision from secret store:
#   /etc/pki/tls/private/server.key

# === Inline-redacted values ===
# These files ARE in the image but have secret values replaced with
# placeholders. Supply actual values at deploy time.
#
#   /etc/wireguard/wg0.conf — PrivateKey (REDACTED_WIREGUARD_KEY_1)
#   /etc/NetworkManager/system-connections/wifi.nmconnection — psk (REDACTED_WIFI_PSK_1)
```

### CLI output

During the render phase, print a summary with remediation context:

```
Secrets handling:
  Excluded (regenerate on target): 3 files
  Excluded (provision from store): 1 file
  Inline-redacted: 2 values in 2 files
  Details: secrets-review.md | Placeholders: redacted/
```

### secrets-review.md format

```markdown
# Secrets Review

The following items were redacted or excluded. Handle them according to
the action specified for each item.

## Excluded Files

| Path | Action | Reason |
|------|--------|--------|
| /etc/cockpit/ws-certs.d/0-self-signed.key | Regenerate on target | Auto-generated cockpit certificate |
| /etc/cockpit/ws-certs.d/0-self-signed.cert | Regenerate on target | Auto-generated cockpit certificate |
| /etc/pki/tls/private/server.key | Provision from secret store | TLS private key |

## Inline Redactions

| Path | Line | Type | Placeholder | Action |
|------|------|------|-------------|--------|
| /etc/wireguard/wg0.conf | 4 | WIREGUARD_KEY | REDACTED_WIREGUARD_KEY_1 | Supply value at deploy time |
| /etc/NetworkManager/.../wifi.nmconnection | 8 | WIFI_PSK | REDACTED_WIFI_PSK_1 | Supply value at deploy time |
```

---

## Testing

### Detection gap tests
- WireGuard `PrivateKey` inline redaction (bare base64, not PEM)
- Container registry `auth.json` full exclusion (under /etc)
- WiFi `psk=` inline redaction
- Binary keystore (`.p12`) full exclusion
- Cockpit `ws-certs.d/` directory exclusion (all files, not just `.key`)
- Sequential counter assignment is deterministic (same input → same counters)
- Sequential counter correlates same secret value across files

### UX tests
- Excluded file written to `redacted/` directory (not `config/`)
- Excluded file has `.REDACTED` suffix
- Excluded file has `include=False` (not in Containerfile COPY)
- Placeholder content varies by remediation state (regenerate vs provision)
- Containerfile has separate comment blocks for excluded vs inline-redacted
- Inline-redacted files ARE still included in Containerfile COPY
- CLI output summary shows correct counts per remediation state
- `secrets-review.md` has separate tables for excluded and inline

### Remediation model tests
- Each pattern maps to correct remediation state
- Regenerate-on-target files say "no action needed" in placeholder
- Provision-from-store files say "provision from secrets management" in placeholder
- Inline-redacted files say "supply value at deploy time" in Containerfile comment

### Provenance and counter tests
- Non-file-backed findings (shadow, container-env, timer-cmd, diff) appear in `secrets-review.md` only — no `.REDACTED` file, no Containerfile entry
- File-backed and non-file-backed findings share counter space (no duplicate counters)
- Counter assignment is deterministic: file-backed first sorted by path/line, then non-file-backed sorted by source/path

### Mixed PEM tests
- Combined cert+key PEM file: private key block is inline-redacted, certificate block preserved
- Cert-only PEM file: no redaction
- Key-only PEM file: full exclusion via `.*\.key$` pattern

---

## Out of Scope

- **Independent heuristic safety net** (entropy analysis, broad PEM detection, binary file detection, known secret filenames): deferred to separate spec. Different precision/recall tradeoff from primary detection.
- **`--include-secrets` opt-in flag**: may be useful eventually, not needed for v2.
- **ostree/rpm-ostree source system handling**: separate workstream (see `comms/threads/2026-04-08-yoinkc-image-mode-source-audit.md`).
- **Inspector scope expansion**: only `/etc/containers/auth.json` is in scope. No inspector changes to scan home directories.

---

## Migration / Backwards Compatibility

- **Output structure**: new `redacted/` top-level directory. Existing `config/` directory no longer contains excluded files. Scripts that parse the config tree by filename may need updates, but yoinkc does not guarantee a stable output API.
- **Placeholder format**: `REDACTED_{TYPE}_{hash}` → `REDACTED_{TYPE}_{N}`. Any downstream tooling matching on hash tokens will need to match on sequential counters instead.
- **`secrets-review.md`**: new format with separate Excluded/Inline tables and remediation guidance. Existing format replaced.
- **Containerfile**: gains comment blocks. No structural changes to COPY directives beyond excluding redacted files.
- **No CLI flag changes.** All new behavior is default-on.
