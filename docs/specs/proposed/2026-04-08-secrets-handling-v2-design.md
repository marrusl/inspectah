# Secrets Handling v2: Detection Gaps, UX, and Safety Net

**Date:** 2026-04-08
**Status:** Proposed
**Author:** Kiwi (orchestrator, synthesizing team input)
**Reviewers:** Kit (code audit), Slate (security assessment), Thorn (test coverage audit), Fern (UX), Ember (product strategy)
**Synthesis thread:** `comms/threads/2026-04-08-yoinkc-secrets-in-output-review.md`

---

## Overview

Harden yoinkc's secrets handling across three workstreams: close detection gaps in the primary pattern set, improve the UX of redacted files so users understand what happened and what to do, and build an independent safety net that catches secret types the primary patterns miss.

## Context

### What exists today

The secrets pipeline has two stages:

1. **Primary detection** (`redact.py`): `redact_snapshot()` runs against the inspection snapshot before rendering. Two mechanisms:
   - `EXCLUDED_PATHS` (6 regex patterns): match file paths, replace content with a 54-byte placeholder (`# Content excluded (sensitive path). Handle manually.`). Currently: `/etc/shadow`, `/etc/gshadow`, `ssh_host_.*`, `/etc/pki/.*\.key`, `.*\.key$`, `.*keytab$`.
   - `REDACT_PATTERNS` (15 content patterns): match within file content, replace matched values with `REDACTED_{TYPE}_{hash}`. Cover PEM private keys, API keys, tokens, passwords, AWS/GitHub/GCP/Azure credentials, database connection strings.

2. **Safety net** (`scan_directory_for_secrets()`): runs post-render against files on disk before git push. Uses the same `REDACT_PATTERNS` list as primary detection.

Redactions are logged to `snapshot.redactions[]` and rendered into `secrets-review.md` in the output.

### What's wrong

A real user ran yoinkc on a Kinoite machine. The output tarball contained cockpit self-signed cert files including a `.key` file. Investigation revealed:

- **The system worked**: the `.key` file content was correctly replaced with a placeholder. No actual key material leaked.
- **The UX is confusing**: the placeholder file still appears in the tarball with its original name, alarming users who see a `.key` file and assume their private key was included.
- **Detection gaps exist**: WireGuard private keys (bare base64, not PEM-wrapped), container registry auth files, WiFi PSKs, and binary keystores are not covered by any pattern.
- **The safety net is redundant**: it uses the same patterns as primary detection, so it catches application bugs but not pattern gaps — the actual threat.
- **The safety net doesn't run on tarballs**: only wired into the git push path. Tarballs are created unconditionally.

---

## Decisions Log

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Two-tier redaction: inline for mixed-content text files, full exclusion for opaque/binary secrets | Maps to user mental model — a WireGuard config "has settings plus a secret" while a `.p12` file "is the secret." Fern confirmed. |
| 2 | Excluded files renamed with `.REDACTED` suffix | Visible in directory listing without opening the file. No false alarm about key leakage. Natural COPY guard. Fern's recommendation. |
| 3 | Excluded files get `include=False` — skipped by Containerfile COPY | Fail-loud on target is correct for a migration tool. Missing key = clear "file not found" error. Placeholder key = runtime mystery. Fern + Ember aligned. |
| 4 | Containerfile gets a comment block listing redacted secrets | Users edit the Containerfile — meet them where they're working. Replaces the need to cross-reference `secrets-review.md` during build. |
| 5 | Independent safety net with broader heuristics | Primary set needs precision (mutates content). Safety net should be deliberately broader and more paranoid (warnings are acceptable). Different precision/recall tradeoffs = different pattern sets. Slate's recommendation. |
| 6 | Safety net runs before tarball creation, not just git push | Tarballs are the primary sharing mechanism. Warnings on tarball, hard block on push. |

---

## Workstream 1: Detection Gaps

### New EXCLUDED_PATHS entries (full-file exclusion → `.REDACTED`)

| Pattern | Rationale |
|---------|-----------|
| `.*\.p12$` | PKCS#12 binary keystore — entirely secret, cannot inline-redact |
| `.*\.pfx$` | Same as `.p12`, Windows naming convention |
| `.*\.jks$` | Java KeyStore — binary, opaque |
| `/etc/cockpit/ws-certs\.d/.*` | Auto-generated, hostname-specific, regenerated on target. Including even the public cert is harmful (wrong hostname/validity). |
| `.*/containers/auth\.json$` | Container registry credentials — base64-encoded auth tokens |
| `.*/\.docker/config\.json$` | Docker registry credentials — same as above |

### New REDACT_PATTERNS entries (inline redaction)

| Pattern name | Regex target | Example match |
|-------------|-------------|---------------|
| `WIREGUARD_KEY` | `PrivateKey\s*=\s*[A-Za-z0-9+/]{43}=` | `PrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5A=` |
| `WIFI_PSK` | `psk\s*=\s*\S+` | `psk=mysecretpassword` |

### Cockpit ws-certs.d behavior

Exclude the entire directory. Cockpit auto-generates self-signed certs on first boot. Carrying them to the target means:
- Wrong hostname on the cert
- Possibly expired validity
- Shared private key across source and target

The correct migration behavior is to let cockpit regenerate certs on the target. The `.REDACTED` placeholder file will tell the user this happened.

---

## Workstream 2: UX Improvements

### File-level behavior for excluded paths

**Current:** Content replaced with placeholder, file written with original name, `include=True`.

**New:**
1. `redact_snapshot()` sets `include=False` on the config entry (so Containerfile COPY skips it)
2. `redact_snapshot()` stores the improved placeholder content (see below) in the entry's `content` field
3. `write_config_tree()` gets a new post-loop pass: for entries with `include=False` that have a redaction record, write the file with `.REDACTED` suffix appended to the original filename. This is a separate code path from the normal config tree write (which checks `include=True`).
4. The `.REDACTED` file is NOT included in Containerfile COPY directives — it exists only in the tarball for user awareness

### Improved placeholder content

```
# REDACTED by yoinkc — sensitive file detected
# Original path: /etc/cockpit/ws-certs.d/0-self-signed.key
# Action required: provision this file on the target system manually
# See secrets-review.md for details and remediation guidance
```

Four lines. Original path for reference. Action required. Pointer to full docs.

### Containerfile comment block

After the existing COPY directives, add a comment block:

```dockerfile
# === Secrets requiring manual provisioning ===
# The following files were detected on the source system but excluded
# from this image because they contain sensitive material:
#
#   /etc/cockpit/ws-certs.d/0-self-signed.key
#   /etc/cockpit/ws-certs.d/0-self-signed.cert
#   /etc/cockpit/ws-certs.d/0-self-signed-ca.pem
#   /etc/wireguard/wg0.conf (PrivateKey redacted inline)
#
# Provision these via your secrets management process after deployment.
# See secrets-review.md for details.
```

This lists both fully-excluded files and inline-redacted files (with a note about what was redacted). Inline-redacted files ARE still COPYed — only excluded files are skipped.

### CLI output

During the render phase, print a summary:

```
Secrets handling:
  Excluded: 3 files (binary/opaque secrets) → see .REDACTED files
  Inline-redacted: 2 values in 1 file (WireGuard key, WiFi PSK)
  See secrets-review.md for full details
```

---

## Workstream 3: Independent Safety Net

### Architecture

`scan_directory_for_secrets()` becomes a separate detection layer with its own heuristics, distinct from the primary `REDACT_PATTERNS`. The primary set is precise (it mutates content). The safety net is deliberately broader (it emits warnings).

### Safety net heuristics

**1. Shannon entropy analysis**

Flag any contiguous run of 40+ high-entropy characters (base64 alphabet: `A-Za-z0-9+/=`, or hex: `0-9a-fA-F`). Config files are mostly low-entropy text. A high-entropy block in a config tree is genuinely suspicious.

Threshold tuning: start conservative. Log warnings, don't block. Adjust based on false positive rate in real tarballs.

**2. Known secret filenames**

Flag files matching known secret names regardless of content:

```
id_rsa, id_ed25519, id_ecdsa, id_dsa
*.p12, *.pfx, *.jks, *.keystore
auth.json, config.json (under .docker/)
htpasswd, .htpasswd
.pgpass, .my.cnf, .netrc
krb5.keytab, *.keytab
```

Many of these overlap with `EXCLUDED_PATHS`. That's fine — defense in depth. The safety net catching something that the primary set should have excluded is itself a signal worth logging.

**3. Broad PEM detection**

The primary pattern matches `BEGIN ... PRIVATE KEY` specifically. The safety net additionally flags any `BEGIN` block:

```
BEGIN CERTIFICATE
BEGIN RSA
BEGIN EC
BEGIN DSA
BEGIN ENCRYPTED
BEGIN PKCS7
BEGIN X509
```

Certificates are not always secret, but in a migration tarball, unexpected PEM material is worth a warning.

**4. Binary file detection**

Any file in the rendered config tree that contains null bytes or fails UTF-8 decode is flagged. Config trees should be text files. Binary content could be keystore files, compiled credentials, or other opaque containers.

### Integration point

Wire `scan_directory_for_secrets()` into `run_pipeline()` before `create_tarball()`:

- **Tarball path:** Run safety net. Emit findings as warnings in CLI output and append to `secrets-review.md` under a "Safety Net Warnings" section. Do not block tarball creation — heuristic matches are not high-confidence enough to hard-block.
- **Git push path:** Keep existing behavior — hard block on any match.

### Warning format in secrets-review.md

```markdown
## Safety Net Warnings

The following items were flagged by heuristic analysis. They may not be
actual secrets, but review them before sharing this tarball.

| File | Heuristic | Detail |
|------|-----------|--------|
| etc/openvpn/client.conf | high-entropy | 44-char base64 block at line 12 |
| etc/pki/java/cacerts | binary-content | non-UTF-8 content detected |
```

---

## Testing

Thorn's audit identified 16 specific tests. Organized by workstream:

### Detection gap tests
- WireGuard `PrivateKey` inline redaction (bare base64, not PEM)
- Container registry `auth.json` full exclusion
- WiFi `psk=` inline redaction
- Binary keystore (`.p12`) full exclusion
- Cockpit `ws-certs.d/` directory exclusion (all files, not just `.key`)

### UX tests
- Excluded file gets `.REDACTED` suffix in output
- Excluded file gets `include=False` (not in Containerfile COPY)
- Placeholder content includes original path and action required
- Containerfile contains secrets comment block listing all redacted items
- Inline-redacted files ARE still included in Containerfile COPY
- CLI output summary counts are correct

### Safety net tests
- Entropy detector fires on high-entropy base64 blocks
- Entropy detector does NOT fire on normal config content
- Known filename detector catches `id_rsa` in config tree
- Broad PEM detector catches `BEGIN CERTIFICATE` (not just `PRIVATE KEY`)
- Binary file detector catches null bytes in config tree
- Safety net runs before tarball creation (integration test)

---

## Out of Scope

- `--include-secrets` opt-in flag: may be useful eventually for users who know they want specific keys, but not needed for v2. Can be added later without design changes.
- Entropy threshold tuning: start conservative, adjust based on real-world data. Not a spec concern.
- ostree/rpm-ostree source system handling: separate workstream (see `comms/threads/2026-04-08-yoinkc-image-mode-source-audit.md`).

---

## Migration / Backwards Compatibility

- Output format changes (`.REDACTED` suffix, Containerfile comments) are additive. Existing scripts that consume yoinkc output by parsing filenames may need updates, but yoinkc does not guarantee a stable output API.
- `secrets-review.md` format gains a new "Safety Net Warnings" section. Existing sections unchanged.
- No CLI flag changes. All new behavior is default-on.
