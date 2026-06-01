# Redaction Pattern Gaps: Four Surgical Fixes

**Status:** Proposed
**Date:** 2026-05-31
**Scope:** `inspectah-pipeline/src/redaction/engine.rs`, `inspectah-pipeline/src/redaction/patterns.rs`

## Context

A parity analysis between the Go redaction engine (`cmd/inspectah/internal/pipeline/redact.go` on `main`) and the Rust engine (`inspectah-pipeline/src/redaction/` on `rust`) identified four pattern-level gaps. Each is a missing behavior the Go code had; none requires new architecture. The existing Rust redaction infrastructure (patterns, engine, `scan_content`, `redact_string`) is correct -- these are additive fixes.

---

## Gap 1: Comment-line filtering

### Problem

Go's `redactText` calls `isCommentLine(out, matchStart)` before processing every regex match. Lines whose trimmed prefix starts with `#`, `//`, or `;` are skipped. The Rust engine has no equivalent -- `redact_string` and `scan_content` process every match regardless of whether it falls on a comment line. This causes false-positive redactions of passwords mentioned in comments (e.g., `# password=old_default`), and can break config file syntax when the redacted token changes the line structure.

### What to change

**File:** `inspectah-pipeline/src/redaction/engine.rs`

Add a helper function `is_comment_line` and call it from both `redact_string` and `scan_content` before processing each match.

### How to implement

```rust
/// Returns true if the match at `pos` falls on a comment line.
/// A comment line is one whose trimmed content (before `pos`) starts
/// with `#`, `//`, or `;`.
fn is_comment_line(content: &str, pos: usize) -> bool {
    let line_start = content[..pos].rfind('\n').map_or(0, |i| i + 1);
    let prefix = content[line_start..pos].trim_start();
    prefix.starts_with('#') || prefix.starts_with("//") || prefix.starts_with(';')
}
```

In `redact_string`, wrap the existing match-processing loop:

```rust
// Inside the `for pat in PATTERNS.iter()` loop, after collecting matches:
for (start, end, matched) in matches.into_iter().rev() {
    if is_comment_line(&result, start) {
        continue;
    }
    let token = registry.token_for(&kind_label, &matched);
    buf.replace_range(start..end, &token);
}
```

In `scan_content`, add the same guard before pushing a finding:

```rust
for mat in pat.regex.find_iter(content) {
    if is_comment_line(content, mat.start()) {
        continue;
    }
    // ... existing finding push
}
```

### What to test

| Test case | Input | Expected |
|---|---|---|
| Hash comment preserved | `# password=old_value\npassword=real` | Comment line untouched, real line redacted |
| Semicolon comment preserved | `; token=example\ntoken=secret` | `;` line untouched |
| C-style comment preserved | `// api_key=docs_example\napi_key=live` | `//` line untouched |
| Inline comment (not a comment line) | `password=secret # old was foo` | Line IS redacted (comment marker is mid-line, not at start) |
| Indented comment | `  # password=old` | Treated as comment (trimmed prefix starts with `#`) |
| First line is comment | `# secret=abc` (no preceding newline) | Treated as comment |

### Risk/complexity

**Low.** Pure additive filter. The function is a string scan to the previous newline -- no allocation, no regex. The only subtlety is ensuring `pos=0` (first line, no newline before it) works, which the `map_or(0, ...)` handles.

---

## Gap 2: Inline PASSWORD_HASH pattern

### Problem

Go defines a standalone regex `\$[1256y]\$[A-Za-z0-9./]+\$[A-Za-z0-9./]+` (typed `PASSWORD_HASH`) that fires on ANY content. This catches crypt hashes in htpasswd files, Kickstart configs, Ansible vault snippets, and anywhere else a `$6$salt$hash` appears.

Rust detects crypt hashes only through `classify_shadow_line()` in `patterns.rs`, which is scoped to `/etc/shadow` paths via the `scan_shadow` function. The `PATTERNS` vec has no `PASSWORD_HASH` entry. Any crypt hash outside a shadow file is invisible.

### What to change

**File:** `inspectah-pipeline/src/redaction/patterns.rs`

Add a `PasswordHash` pattern entry to the `PATTERNS` vec.

### How to implement

Add a new `SecretPattern` entry to the `PATTERNS` `LazyLock` vec, after the existing `Password` entry:

```rust
// Crypt password hashes ($1$, $5$, $6$, $y$) anywhere in content.
// Shadow files are handled separately by scan_shadow, but hashes
// appear in htpasswd, kickstart, ansible, and other configs.
SecretPattern {
    regex: Regex::new(r"\$[1256y]\$[A-Za-z0-9./]+\$[A-Za-z0-9./]+").unwrap(),
    finding_kind: FindingKind::PasswordHash,
    detection_method: DetectionMethod::Pattern,
    confidence: Confidence::High,
    remediation: "Remove password hash or use a secrets manager",
},
```

`FindingKind::PasswordHash` already exists in the enum. No type changes needed.

**Ordering note:** Place this BEFORE the generic `Password` pattern. The crypt hash regex is more specific and should match first, preventing the generic `password=` pattern from partially matching a hash that happens to follow a `password=` key.

### What to test

| Test case | Input | Expected |
|---|---|---|
| SHA-512 crypt hash in htpasswd | `admin:$6$rounds=5000$salt$longhash` | `PasswordHash` finding, hash redacted |
| yescrypt hash in kickstart | `rootpw --iscrypted $y$j9T$salt$hash` | `PasswordHash` finding |
| MD5 crypt hash standalone | `$1$abc$def` | `PasswordHash` finding |
| SHA-256 crypt hash | `$5$salt$hash` | `PasswordHash` finding |
| Non-crypt dollar signs | `$HOME` or `$PATH` | No match (no `$N$` prefix with valid algo digit) |
| Dollar in shell variable | `FOO=$BAR` | No match |
| Shadow path still uses scan_shadow | `/etc/shadow` content | Both `scan_shadow` AND pattern fire; deduplicate or accept dual findings |

**Shadow overlap note:** With this pattern, crypt hashes in `/etc/shadow` will produce findings from both `scan_shadow` (which classifies locked/disabled/hash semantics) AND the generic pattern. Two options:

- **Option A (recommended):** Skip `PasswordHash` pattern matches when `path` is a shadow file. Add a `skip_paths` field or a path check in `scan_content`. Keeps `scan_shadow`'s richer classification as the authority for shadow files.
- **Option B:** Accept dual findings. Simpler but slightly noisier.

Recommend Option A: in `scan_content`, when the path is a shadow file, filter out `PasswordHash` findings from the generic pattern pass to avoid duplicates.

### Risk/complexity

**Low.** Single pattern addition to an existing vec. The regex is identical to Go's proven pattern. The only consideration is the shadow overlap, handled by a simple path check.

---

## Gap 3: PEM block matching is partial

### Problem

Go's PEM patterns use `(?s)` (dot-matches-newline) to capture the entire `BEGIN...END` block including the base64-encoded key material between the markers:

```
(?s)-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----
```

Rust's pattern matches only the `BEGIN` header line:

```
-----BEGIN\s+(?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----
```

This means `redact_string` replaces only the header with a token, leaving the actual key material (the base64 lines and `END` marker) in the output. The finding is recorded, but the secret is not removed.

### What to change

**File:** `inspectah-pipeline/src/redaction/patterns.rs`

Replace the `PrivateKey` regex with a multi-line block pattern.

### How to implement

The `regex` crate does not support `(?s)` (dot-matches-newline) by default. Use the inline flag `(?s)` which the `regex` crate DOES support:

```rust
SecretPattern {
    regex: Regex::new(
        r"(?s)-----BEGIN\s+(?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----.*?-----END\s+(?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----"
    ).unwrap(),
    finding_kind: FindingKind::PrivateKey,
    detection_method: DetectionMethod::Pattern,
    confidence: Confidence::High,
    remediation: "Remove private key or exclude file from snapshot",
},
```

Also add the `Certificate` block pattern that Go has (currently absent from Rust's `PATTERNS`):

```rust
SecretPattern {
    regex: Regex::new(
        r"(?s)-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----"
    ).unwrap(),
    finding_kind: FindingKind::Certificate,
    detection_method: DetectionMethod::Pattern,
    confidence: Confidence::High,
    remediation: "Certificates are public data but may indicate key material nearby",
},
```

`FindingKind::Certificate` already exists in the enum.

### What to test

| Test case | Input | Expected |
|---|---|---|
| RSA private key block | Full `BEGIN RSA PRIVATE KEY` ... base64 ... `END RSA PRIVATE KEY` | Entire block replaced with single `REDACTED_PRIVATEKEY_1` token |
| EC private key block | `BEGIN EC PRIVATE KEY` ... `END EC PRIVATE KEY` | Entire block redacted |
| OPENSSH private key | `BEGIN OPENSSH PRIVATE KEY` ... `END OPENSSH PRIVATE KEY` | Entire block redacted |
| Mixed PEM bundle | Certificate block + private key block | Only private key block redacted; certificate block gets a finding but content preserved (certificates are public) |
| Header-only (no END marker) | `-----BEGIN RSA PRIVATE KEY-----\ndata` (truncated, no END) | No match -- avoids greedy consumption of unrelated content |
| Adjacent PEM blocks | Two private key blocks in sequence | Each matched independently (`.*?` is non-greedy) |

### Risk/complexity

**Low-medium.** The regex change is straightforward, but verify that `(?s)` works correctly with the `regex` crate (it does -- `(?s)` is a supported inline flag). The non-greedy `.*?` is critical to prevent matching across unrelated blocks. Test with adjacent blocks to confirm.

The `redact_string` function's replace-from-end-to-start strategy handles multi-line replacements correctly since it operates on byte offsets. No structural changes needed to the replacement loop.

---

## Gap 4: Value-level false-positive filtering

### Problem

Go checks matched PASSWORD values against a set of 20 known NSS/PAM tokens before redacting:

```go
var falsePositiveValues = map[string]bool{
    "files": true, "compat": true, "sss": true, "ldap": true,
    "nis": true, "hesiod": true, "systemd": true, "nisplus": true,
    "winbind": true, "required": true, "sufficient": true,
    "optional": true, "include": true, "substack": true,
    "pam_unix.so": true, "pam_sss.so": true, "pam_deny.so": true,
    "pam_permit.so": true, "pam_env.so": true, "requisite": true,
}
```

This fires for ANY path. When a match has `typeLabel == "PASSWORD"` and the captured value is in this set, the match is skipped.

Rust takes a coarser approach -- `REDACTION_ALLOWLIST` skips the entire `etc/pam.d/` path prefix. This means:

1. `/etc/nsswitch.conf` with `passwd: files sss` -- Go skips it (value `files` is allowlisted), Rust redacts it (path not in allowlist).
2. Any file outside `etc/pam.d/` containing PAM-style tokens -- Go skips, Rust redacts.
3. Files inside `etc/pam.d/` are blanket-skipped in Rust even if they contain real secrets.

### What to change

**File:** `inspectah-pipeline/src/redaction/engine.rs`

Replace the path-based allowlist with a value-based false-positive check, matching Go's approach. Keep the path allowlist as a secondary defense but make the value check the primary filter.

### How to implement

Add a static set of known false-positive values:

```rust
use std::collections::HashSet;

/// Known non-secret values that appear after `password:` or `passwd:`
/// in NSS, PAM, and similar config files. Checked case-insensitively.
static FALSE_POSITIVE_VALUES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "files", "compat", "sss", "ldap", "nis", "hesiod",
        "systemd", "nisplus", "winbind", "required", "sufficient",
        "optional", "include", "substack", "pam_unix.so",
        "pam_sss.so", "pam_deny.so", "pam_permit.so",
        "pam_env.so", "requisite",
    ].into_iter().collect()
});

/// Returns true if `value` is a known NSS/PAM token, not a real secret.
fn is_false_positive_value(value: &str) -> bool {
    FALSE_POSITIVE_VALUES.contains(value.trim().to_lowercase().as_str())
}
```

In `redact_string`, after matching a `Password` pattern, extract the value portion and check:

```rust
for (start, end, matched) in matches.into_iter().rev() {
    if is_comment_line(&result, start) {
        continue;
    }
    // For Password patterns, check if the value portion is a known
    // false positive (NSS/PAM token).
    if pat.finding_kind == FindingKind::Password {
        // Value is everything after the `=` or `:` separator.
        if let Some(sep_pos) = matched.find('=').or_else(|| matched.find(':')) {
            let value = &matched[sep_pos + 1..];
            if is_false_positive_value(value) {
                continue;
            }
        }
    }
    let token = registry.token_for(&kind_label, &matched);
    buf.replace_range(start..end, &token);
}
```

Apply the same guard in `scan_content`:

```rust
for mat in pat.regex.find_iter(content) {
    if is_comment_line(content, mat.start()) {
        continue;
    }
    if pat.finding_kind == FindingKind::Password {
        let matched = mat.as_str();
        if let Some(sep_pos) = matched.find('=').or_else(|| matched.find(':')) {
            let value = &matched[sep_pos + 1..];
            if is_false_positive_value(value) {
                continue;
            }
        }
    }
    // ... existing finding push
}
```

**Keep `REDACTION_ALLOWLIST`** as a secondary defense for `etc/pam.d/` -- it catches edge cases where PAM configs use non-standard tokens. But the value check is now the primary filter and works for all paths.

### What to test

| Test case | Input (path, content) | Expected |
|---|---|---|
| nsswitch.conf with NSS tokens | `/etc/nsswitch.conf`, `passwd: files sss` | No PASSWORD finding (both `files` and `sss` are false positives) |
| PAM config outside pam.d | `/etc/security/pwquality.conf`, `password requisite pam_pwquality.so` | No finding for `requisite` |
| Real password after key | `/etc/app.conf`, `password=s3cret` | PASSWORD finding (value `s3cret` not in allowlist) |
| Mixed line | `/etc/nsswitch.conf`, `passwd: files\ndb_password=real` | First line skipped, second line redacted |
| Case insensitivity | `password: FILES` | No finding (case-insensitive check) |
| pam.d still allowlisted | `/etc/pam.d/system-auth`, `password sufficient pam_unix.so` | No finding (both value filter AND path filter apply) |
| PAM module with real secret | `/etc/pam.d/custom`, `password=actualpass123` | Redacted (value `actualpass123` not in false-positive set; path allowlist should NOT suppress real secrets -- this is a behavior change from current Rust) |

**Behavior change note:** The last test case reveals that the current `etc/pam.d/` path allowlist suppresses ALL findings in that directory, including real secrets. With the value-based approach, real secrets in `pam.d/` files will now be correctly detected. This is the desired behavior.

### Risk/complexity

**Low-medium.** The value extraction depends on the PASSWORD regex's structure -- it must contain `=` or `:` as a separator. The current Rust regex `(?i)(?:password|passwd|...)\\s*[=:]\\s*\\S+` guarantees this. If the regex changes in the future, the separator extraction must stay in sync. A comment in the code should note this coupling.

---

## Implementation order

1. **Gap 1 (comment filtering)** -- standalone, no dependencies. Add `is_comment_line` and wire it.
2. **Gap 4 (false-positive values)** -- depends on Gap 1 being in place (both are guards in the same match loop). Add `FALSE_POSITIVE_VALUES` and the value check.
3. **Gap 2 (inline PASSWORD_HASH)** -- standalone pattern addition. Add after Gaps 1 and 4 so the new pattern benefits from comment filtering.
4. **Gap 3 (PEM block matching)** -- standalone regex fix. No dependencies but test last since it changes match spans from single-line to multi-line.

## Estimated effort

All four gaps are implementable in a single session. Each gap is 10-30 lines of logic + 30-60 lines of tests. Total: ~200 lines of production code, ~300 lines of tests.
