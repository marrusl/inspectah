# Heuristic Secrets Safety Net

**Status:** Approved (2026-04-09, review nits folded in)
**Date:** 2026-04-08
**Related:** `2026-04-08-secrets-handling-v2-design.md` (pattern-based redaction improvements)

## Relationship to v2

This spec extends, not replaces, the approved v2 secrets handling spec. All v2 contracts remain normative:

- **`RedactionFinding` dataclass:** This spec adds `detection_method` and `confidence` fields to v2's typed model. All other v2 fields (`path`, `source`, `kind`, `pattern`, `remediation`, `line`, `replacement`) are unchanged.
- **Containerfile comment blocks:** v2's two-block structure (excluded secrets vs inline-redacted values) is preserved exactly. This spec adds one additional line to the inline block noting heuristic detection count when applicable.
- **Remediation states:** v2's three-state model (regenerate on target, provision from secret store, value removed inline) is the only remediation vocabulary. This spec does not introduce new remediation language. Heuristic inline redactions use "value removed inline." Heuristic flagged-only findings use no remediation state — they are advisory.
- **Sequential counters:** Heuristic redactions participate in v2's global counter space (`REDACTED_{TYPE}_{N}`). Ordering: pattern findings first (v2's deterministic order: file-backed by path/line, then non-file-backed by source/path), then heuristic findings (same sort: file-backed by path/offset, then non-file-backed by source/path). Flagged-only findings (low-confidence in strict mode, all heuristic findings in moderate mode) do not consume counters — they have no replacement token.
- **`secrets-review.md` structure:** v2's separate Excluded Files and Inline Redactions tables are preserved. This spec adds a third table: "Flagged for Review" for heuristic advisory findings. The summary line adds heuristic counts.
- **CLI summary:** v2 groups by remediation state. This spec adds a supplementary heuristic line beneath v2's summary rather than replacing it.

Where this spec and v2 would conflict, v2 wins unless explicitly amended.

## Problem

yoinkc's pattern-based redaction catches secrets with known formats (PEM keys, AWS keys, GitHub tokens, connection strings, etc.). But secrets without a known regex shape — custom API keys, generated passwords in config files, embedded tokens from less-common vendors — pass through undetected. The safety net is a second detection layer that uses heuristic signals (entropy analysis, keyword proximity) to catch what patterns miss.

## Architecture

**Post-redaction, Approach A.** The heuristic layer runs after pattern-based redaction, evaluating only content that survived the pattern pass. This means a smaller corpus, fewer false positives, and clean defense-in-depth layering.

**Separate module.** The heuristic engine lives in its own module (not inline in `redact.py`). Pattern matching and heuristic scoring have fundamentally different tuning characteristics — entropy thresholds, keyword dictionaries, scoring weights. Isolating them makes testing and threshold tuning cleaner.

**Single finding type.** Heuristic findings use the same `RedactionFinding` dataclass as pattern findings, extended with two new fields:

- `detection_method: str` — `"pattern"` | `"heuristic"` | `"excluded_path"`
- `confidence: Optional[str]` — `"high"` | `"low"` | `None` (None for pattern findings)

`detection_method: "excluded_path"` is distinct from v2's `kind: "excluded"`. The `kind` field describes what happened to the file (excluded vs inline-redacted). The `detection_method` field describes how the finding was detected (pattern match, heuristic scoring, or path-based exclusion). Both fields are present on every finding.

Renderers consume findings uniformly regardless of detection method. The distinction surfaces only in audit output (`secrets-review.md`) and CLI summary counts.

How the heuristic module is wired into the pipeline (call site, internal structure) is an implementation decision, not a spec concern.

## Pattern Layer Additions

The following vendor token prefix patterns are added to the pattern-based redaction layer. These are deterministic (not heuristic) — they have unique, well-documented prefixes that make regex matching reliable.

**Regex notation:** Tables below use Markdown-escaped pipes (`\|`) for readability. In Python implementation, alternation is bare `|` — do not copy `\|` literally into `re.compile()`. Each new pattern must have at least one positive and one negative test vector validated against vendor documentation before merge.

### Fixing existing coverage

The bare `sk-` prefix proposed in the v2 spec is too broad. Replace with vendor-specific patterns:

| Prefix | Service | Label | Regex |
|--------|---------|-------|-------|
| `sk_live_`, `sk_test_`, `rk_live_`, `rk_test_` | Stripe | `STRIPE_KEY` | `(?:sk\|rk)_(?:test\|live)_[a-zA-Z0-9]{10,99}` |
| `sk-ant-api03-`, `sk-ant-admin01-` | Anthropic | `ANTHROPIC_KEY` | `sk-ant-(?:api03\|admin01)-[a-zA-Z0-9_\-]{80,}` |
| `sk-proj-`, `sk-svcacct-` | OpenAI | `OPENAI_KEY` | `sk-(?:proj\|svcacct\|admin)-[A-Za-z0-9_-]{20,}` |

**Note on OpenAI:** The OpenAI key format has changed multiple times. The regex above is intentionally broader than the `T3BlbkFJ`-marker pattern from earlier research, trading some false-positive risk for resilience to format changes. Validate against current vendor docs at implementation time. Flag as "best effort — vendor format may evolve."

### New Tier 1 patterns (high priority, commonly seen on RHEL servers)

| Prefix | Service | Label | Regex |
|--------|---------|-------|-------|
| `ASIA`, `ABIA`, `ACCA` | AWS (temp session keys) | `AWS_TEMP_KEY` | `(?:A3T[A-Z0-9]\|ASIA\|ABIA\|ACCA)[A-Z2-7]{16}` |
| `github_pat_` | GitHub (fine-grained PAT) | `GITHUB_TOKEN` | `github_pat_[a-zA-Z0-9_]{36,255}` |
| `ghs_` | GitHub (app installation) | `GITHUB_TOKEN` | `ghs_[0-9a-zA-Z]{36}` |
| `gho_` | GitHub (OAuth) | `GITHUB_TOKEN` | `gho_[a-zA-Z0-9]{36}` |
| `sha256~` | OpenShift (OAuth token) | `OPENSHIFT_TOKEN` | `sha256~[\w-]{43}` |
| `hvs.` | HashiCorp Vault (service) | `VAULT_TOKEN` | `hvs\.[a-zA-Z0-9_-]{24,}` |
| `hvb.` | HashiCorp Vault (batch) | `VAULT_TOKEN` | `hvb\.[\w-]{138,300}` |
| `glpat-` | GitLab (personal access) | `GITLAB_TOKEN` | `glpat-[a-zA-Z0-9_-]{20,}` |
| `glrt-` | GitLab (runner) | `GITLAB_TOKEN` | `glrt-[0-9a-zA-Z_\-]{20}` |
| `gldt-` | GitLab (deploy) | `GITLAB_TOKEN` | `gldt-[0-9a-zA-Z_\-]{20}` |
| `glptt-` | GitLab (pipeline trigger) | `GITLAB_TOKEN` | `glptt-[0-9a-f]{40}` |
| `xoxb-`, `xoxp-` | Slack (bot/user token) | `SLACK_TOKEN` | `xox[bp]-[a-zA-Z0-9-]{24,}` |
| `SG.` | SendGrid | `SENDGRID_KEY` | `SG\.[a-zA-Z0-9_-]{22,}` |
| `dapi` | Databricks | `DATABRICKS_TOKEN` | `dapi[a-f0-9]{32}(?:-\d)?` |
| `ATATT3` | Atlassian | `ATLASSIAN_TOKEN` | `ATATT3[A-Za-z0-9_\-=]{186}` |
| `AKCp` | Artifactory | `ARTIFACTORY_KEY` | `AKCp[A-Za-z0-9]{69}` |
| `LTAI` | Alibaba Cloud | `ALIBABA_KEY` | `LTAI[a-zA-Z0-9]{20}` |
| `npm_` | npm registry | `NPM_TOKEN` | `npm_[a-zA-Z0-9]{36}` |
| `pypi-AgEIcHlwaS5vcmc` | PyPI (upload token) | `PYPI_TOKEN` | `pypi-AgEIcHlwaS5vcmc[\w-]{50,1000}` |
| `rubygems_` | RubyGems | `RUBYGEMS_TOKEN` | `rubygems_[a-f0-9]{48}` |
| `AGE-SECRET-KEY-` | age encryption | `AGE_KEY` | `AGE-SECRET-KEY-1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{58}` |

### New Tier 2 patterns (enterprise/DevOps environments)

| Prefix | Service | Label | Regex |
|--------|---------|-------|-------|
| `dop_v1_` | DigitalOcean (personal) | `DIGITALOCEAN_TOKEN` | `dop_v1_[a-f0-9]{64}` |
| `doo_v1_` | DigitalOcean (OAuth) | `DIGITALOCEAN_TOKEN` | `doo_v1_[a-f0-9]{64}` |
| `HRKU-AA` | Heroku | `HEROKU_KEY` | `HRKU-AA[0-9a-zA-Z_-]{58}` |
| `glc_` | Grafana Cloud | `GRAFANA_TOKEN` | `glc_[A-Za-z0-9+/]{32,400}={0,3}` |
| `glsa_` | Grafana (service account) | `GRAFANA_TOKEN` | `glsa_[A-Za-z0-9]{32}_[A-Fa-f0-9]{8}` |
| `NRAK-` | New Relic (user key) | `NEWRELIC_KEY` | `NRAK-[a-z0-9]{27}` |
| `NRII-` | New Relic (insight insert) | `NEWRELIC_KEY` | `NRII-[a-z0-9-]{32}` |
| `sntrys_` | Sentry (org auth) | `SENTRY_TOKEN` | `sntrys_eyJpYXQiO[A-Za-z0-9+/=_-]{80,}` |
| `dp.pt.` | Doppler | `DOPPLER_TOKEN` | `dp\.pt\.[a-z0-9]{43}` |
| `pul-` | Pulumi | `PULUMI_TOKEN` | `pul-[a-f0-9]{40}` |

Services with generic tokens (no unique prefix) — CircleCI, Datadog, Jenkins, Travis CI, Codecov, Drone — are not suitable for pattern matching and are deferred to the heuristic layer.

The full research backing these patterns is at `marks-inbox/research/2026-04-08-vendor-token-prefix-landscape.md`.

## Heuristic Detection Model

### Signals

| Signal | Weight | Description |
|--------|--------|-------------|
| Keyword proximity | **Strong** | Value appears on the same line or in the same key-value pair as a secret keyword (`password`, `passwd`, `secret`, `token`, `api_key`, `credential`, `auth`, `private_key`, etc.) |
| Shannon entropy | **Strong** | Value exceeds ~4.5 bits/char entropy threshold (tunable). Calculated via sliding window over candidate values. Threshold should be tunable per charset class — hex strings have lower max entropy (~4.0) than base64 (~6.0) or mixed alphanumeric. |
| Vendor prefix (residual) | **Strong** | Catches strings matching a `[a-zA-Z]{2,}_[a-zA-Z0-9]{20,}` shape (short alphabetic prefix + underscore + long random suffix) that weren't caught by the pattern layer. When a residual prefix shows up repeatedly across runs, it should be identified and graduated to the pattern layer. |
| Value length | **Weak** | Value is 20-128 characters. Corroborating signal only — long strings are common for non-secrets. |
| Assignment context | **Weak** | Value follows `=`, `:`, or appears in a structured key-value pair. Corroborating signal only — most config values use these. |

### Confidence rules

- **High confidence:** 1 strong signal + any corroborating signal, OR 2+ strong signals
- **Low confidence:** 1 strong signal alone, OR weak-only signal combinations

Examples:
- `db_password = aR$9xk!mQ2pL` — keyword proximity (strong) + entropy (strong) = **high**
- `VAULT_TOKEN=hvs.CAESIJx8ZtLR...` — caught by pattern layer, never reaches heuristics
- `session_timeout = 3600` — assignment context (weak) + length too short = **no finding**
- `config_key = a8f2b9c4d5e6...` (64 hex chars) — entropy (strong) alone = **low**
- `secret = false` — keyword (strong) but value is boolean, false positive filter catches it = **no finding**

### False positive mitigation

- Skip values already containing `REDACTED_` (handled by pattern pass)
- Skip comment lines (reuse existing `_is_comment_line()`)
- Exempt known high-entropy non-secret formats:
  - UUIDs (8-4-4-4-12 hex pattern)
  - Hex checksums (exactly 32, 40, or 64 hex chars with no mixed case)
  - Boolean/numeric values following secret keywords (`secret = false`, `password_min_length = 12`)

### Noise control

To prevent operator fatigue from drowning real findings in false positives:

**Caps limit reporting only.** All heuristic findings — including those beyond the cap — are still evaluated for redaction (in strict mode) and push-block decisions. The caps suppress advisory output, not detection or enforcement.

- **Per-file cap:** Maximum 10 heuristic findings per file. If a file exceeds the cap, report the first 10 and append a summary line: "N additional heuristic findings suppressed in this file."
- **Per-run cap:** Maximum 100 heuristic findings per run. Beyond the cap, suppress additional findings and note the count in the CLI summary and `secrets-review.md`.
- **Deduplication:** Identical values found at multiple paths are reported once with a "also found in N other locations" note, not N separate findings. The primary row is the earliest location by the standard finding sort order (file-backed by path/line first, then non-file-backed by source/path).
- **Residual prefix graduation:** When a residual prefix pattern triggers 3+ times across captured configs in a single run, log it as a candidate for promotion to the pattern layer. Include the prefix and count in `secrets-review.md` under a "Pattern candidates" note.

**Ordering:** Caps and dedup interact as follows: (1) collect all heuristic findings, (2) deduplicate (collapse identical values, keep the primary row per sort order), (3) apply per-file and per-run caps to the deduplicated list in sort order. The "first 10" per file and "first 100" per run are determined by the standard finding sort order (file-backed by path/line, then non-file-backed by source/path).

These caps should be tunable via implementation constants, not CLI flags. Adjust based on real-world false-positive rates from pilot runs.

## Sensitivity Levels

### `--sensitivity strict|moderate`

Default: `strict`.

| Level | Pattern redaction | High-confidence heuristic | Low-confidence heuristic |
|-------|-------------------|---------------------------|--------------------------|
| **strict** | Redact | Redact | Flag |
| **moderate** | Redact | Flag | Flag |

The only behavioral difference between strict and moderate is whether high-confidence heuristic findings are redacted or flagged. Pattern-based redaction runs in both modes.

**Moderate mode warning:** In moderate mode, all heuristic-detected secrets remain unredacted in output artifacts. Because only flagged findings (not redacted findings) result from heuristic detection in this mode, push will not be blocked by heuristic findings. Operators choosing moderate mode accept the risk that real secrets detected by heuristics will pass through to output unredacted.

### `--no-redaction`

Separate switch, orthogonal to `--sensitivity`. Disables all redaction — pattern and heuristic detection still runs, all findings are flagged but no content is modified.

**Mutual exclusion:** If `--sensitivity` and `--no-redaction` are both passed, exit with error: `--sensitivity has no effect when --no-redaction is set`.

**Completion warning when `--no-redaction` is used:**

```
WARNING: Redaction was disabled for this run.
Output may contain passwords, tokens, API keys, and other secrets.

  5 pattern findings were NOT redacted
  3 high-confidence heuristic findings were NOT redacted
  4 low-confidence heuristic findings flagged

See secrets-review.md for the full list of detected secrets.
Do not share, commit, or upload this output without manual review.
```

The warning quantifies what was skipped (making the risk concrete) and gives an actionable instruction. No confirmation prompts or env var gates — `--no-redaction` is self-documenting and the target audience is sysadmins who know what they typed.

## Output Surfaces

All output surfaces preserve v2's structure and remediation vocabulary. Heuristic findings are additive.

### CLI summary

v2's remediation-grouped summary remains primary. Heuristic counts are appended as a supplementary line:

```
Detected secrets:
  Excluded (regenerate on target): 3 files
  Excluded (provision from store): 1 file
  Inline-redacted: 5 values in 3 files (2 pattern, 3 heuristic)
  Flagged for review: 4 heuristic findings
  Details: secrets-review.md | Placeholders: redacted/
```

Detection is best-effort. CLI help text should include: "Review secrets-review.md before distributing output. Detection covers known patterns and heuristic signals but is not exhaustive."

### Containerfile

v2's two-block structure (excluded secrets vs inline-redacted values) is preserved exactly. Heuristic inline redactions appear in the inline block alongside pattern redactions — they are not distinguished in the Containerfile (both were redacted; the operator action is the same). If any heuristic findings were flagged but not redacted, add one line after the inline block:

```dockerfile
# === Excluded secrets (not in this image) ===
# [v2 content exactly as specified]

# === Inline-redacted values ===
# [v2 content, including any heuristic-redacted values]

# Note: 4 additional values were flagged for review but not redacted.
# See secrets-review.md for details.
```

If no heuristic findings were flagged (all were redacted or no heuristic hits), the note line is omitted.

### `secrets-review.md`

v2's Excluded Files and Inline Redactions tables are preserved. This spec adds a third table for heuristic advisory findings:

```markdown
# Secrets Review

> Detected secrets: 9 redacted (6 pattern, 3 heuristic), 4 flagged for review

## Excluded Files

| Path | Action | Reason |
|------|--------|--------|
| /etc/cockpit/ws-certs.d/0-self-signed.key | Regenerate on target | Auto-generated cockpit certificate |
| /etc/pki/tls/private/server.key | Provision from secret store | TLS private key |

## Inline Redactions

| Path | Line | Type | Placeholder | Detection | Action |
|------|------|------|-------------|-----------|--------|
| /etc/wireguard/wg0.conf | 4 | WIREGUARD_KEY | REDACTED_WIREGUARD_KEY_1 | pattern | Supply value at deploy time |
| /etc/myapp/config.ini | 12 | PASSWORD | REDACTED_PASSWORD_3 | heuristic (high) | Supply value at deploy time |

## Flagged for Review

These values were detected by heuristic analysis but not redacted. Review
manually and handle as needed.

| Path | Line | Confidence | Why Flagged |
|------|------|------------|-------------|
| /etc/sysconfig/app.conf | 8 | low | High entropy value (5.2 bits/char) near "config_key" |
| containers:running/myapp:env | — | low | Keyword "token" with 28-char alphanumeric value |
```

Non-file-backed heuristic findings (container env, timer commands) appear in `secrets-review.md` only — consistent with v2's `source` rules. They do not produce `.REDACTED` files or Containerfile entries.

**`--no-redaction` mode header:**

```markdown
> WARNING: Redaction was disabled for this run. All values listed below
> appear unredacted in the output artifacts.
```

All rows show "Not redacted" in the Action column.

## Output Verification

`scan_directory_for_secrets()` currently runs pattern-only against rendered output before push. This spec extends it:

- **Heuristic scan on output tree:** After rendering, run the heuristic engine against files in the output directory using the same sensitivity level as the main run. This catches secrets that survived both pattern and heuristic passes on the snapshot (e.g., values introduced by rendering templates).
- **Subscription certificate exclusion:** The output tree scan must skip `entitlement/` and `rhsm/` directories under the output root. These contain bundled subscription certificates that are required for builds and are managed by a separate pathway. The exclusion applies to both pattern and heuristic scanning of the output tree.
- **Flagged findings do not block push.** Only redacted-tier findings (pattern hits + high-confidence heuristic in strict mode) that survive in the output tree trigger a push block. Flagged-only findings are reported but do not prevent the operation.

## Subscription Certificate Exclusion

RHEL subscription certificates (typically under `/etc/pki/entitlement/` and `/etc/rhsm/`) contain PEM key material that will trigger both pattern and heuristic detection. However, these certificates are required for package installation during image builds on non-RHEL hosts.

Subscription certificates are handled by `subscription.py`'s `bundle_subscription_certs()` function, which copies PEM files from `/etc/pki/entitlement/` and rhsm config from `/etc/rhsm/` into the output directory. This runs in `pipeline.py` as a separate step from the redaction pass, gated by `--no-subscription`. Both the snapshot-level heuristic pass and the output-tree verification scan must skip content managed by this pathway.

**Snapshot-level exclusion:** Skip any `ConfigFileEntry` whose path starts with `/etc/pki/entitlement/` or `/etc/rhsm/`. These paths are owned by `bundle_subscription_certs()` and never pass through the redaction pipeline — pattern or heuristic.

**Output-tree exclusion:** Skip `entitlement/` and `rhsm/` directories under the output root during `scan_directory_for_secrets()`.

**Implementation note:** The `--no-subscription` flag controls whether certs are bundled into output, but does not affect the exclusion — even when subscription bundling is skipped, these paths should still be excluded from heuristic scanning (the PEM content in the snapshot would still trigger false positives).

## Testing

### Pattern tests
- Each new vendor prefix pattern has at least one positive match test (from vendor docs or public examples) and one negative test (similar-looking non-secret string)
- Regex correctness: all patterns compile without error; no `\|` literals where alternation `|` is intended

### Heuristic detection tests
- High confidence: keyword + entropy → redacted in strict, flagged in moderate
- Low confidence: entropy alone → flagged in strict, flagged in moderate
- False positive filters: UUID passes through, hex checksum passes through, `secret = false` passes through, `password_min_length = 12` passes through

### Sensitivity level tests
- `strict` default: pattern findings redacted, high-confidence heuristic redacted, low-confidence flagged
- `moderate`: pattern findings redacted, all heuristic findings flagged
- `--no-redaction`: all detection runs, nothing redacted, all findings flagged, completion warning printed
- `--sensitivity` + `--no-redaction`: exits with error message

### Counter ordering tests
- Pattern findings get counters first, heuristic findings get subsequent counters
- Same input produces same counter assignments across runs (determinism)
- Flagged-only findings do not consume counters
- File-backed and non-file-backed findings share counter space (no duplicates)

### Output surface tests
- `secrets-review.md` has three tables: Excluded Files, Inline Redactions, Flagged for Review
- Containerfile preserves v2's two-block structure; flagged note appears only when applicable
- CLI summary shows remediation-grouped counts with heuristic supplement line
- `--no-redaction` mode: warning header in `secrets-review.md`, "Not redacted" in Action columns

### Noise control tests
- File with >10 heuristic hits: only 10 reported, suppression note present
- Run with >100 heuristic hits: cap applied, summary notes total suppressed
- Duplicate values across files: reported once with location count

### Output verification tests
- Heuristic scan runs on output tree after rendering
- Subscription cert directories (`entitlement/`, `rhsm/`) skipped in output scan
- Flagged findings in output tree do not block push
- Pattern/high-confidence findings in output tree do block push

## Out of Scope

- **Subscription certificate inclusion default** — separate decision, separate spec if changed
- **Interactive override workflow** (e.g., per-finding allow/deny prompts) — not needed for v1
- **Custom user-defined patterns** — possible future extension, not needed now
- **Network-based secret detection** (e.g., calling external scanning APIs) — yoinkc is offline-capable
- **Binary keystore detection** (`.p12`, `.jks`, etc.) — handled by v2's excluded paths, not heuristic layer
