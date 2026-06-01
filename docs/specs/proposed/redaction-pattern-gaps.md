# Redaction Pattern Gaps

**Status:** Proposed (revision 5 — final)
**Date:** 2026-06-01
**Scope:** `inspectah-pipeline/src/redaction/engine.rs`, `inspectah-pipeline/src/redaction/patterns.rs`
**Revision notes (r5):** Fixes internal contradiction between callsite snippets and dedup architecture. The early Before/After examples (callsite changes, conversion table) showed per-pattern `collect_eligible_matches` calls inside existing per-pattern loops, contradicting the dedup section which requires collecting matches across ALL patterns first. Rewritten to show the all-patterns-first collection + dedup flow consistently. Helper signature updated to include `path: Option<&str>` throughout.
**Prior revision notes (r4):** Addresses round-3 review findings. (1) Step 1 behavior-neutral claim corrected — dedup is global and changes existing Password/JdbcPassword overlap semantics. Existing overlaps documented with regression test requirements. (2) Two vacuous Gap 4 test rows replaced — `password requisite ...` and `password sufficient ...` never trigger the Password regex (no `=` or `:` separator), so they didn't exercise the false-positive filter. Replaced with inputs that actually reach the filter code path.
**Prior revision notes (r3):** Complete callsite inventory, `REDACTION_ALLOWLIST` behavior corrected, PasswordHash/Password overlap dedup rule added.
**Prior revision notes (r2):** Structural helper for mutation-path consistency (Gaps 1 & 4), regex re-derivation (Gap 2), certificate redaction approach clarified (Gap 3). Scope upgraded from "surgical" to "structural refactor + pattern additions."

## Context

A parity analysis between the Go redaction engine and the Rust engine (`inspectah-pipeline/src/redaction/` on `rust`) identified four pattern-level gaps. Each is a missing behavior the Go code had.

**Scope correction (from r1 review):** The original spec described these as "surgical fixes" to `scan_content()` and `redact_string()`. That understated the required change. The file-level `redact()` function in `engine.rs` has its own replacement loop that calls `scan_content()` to generate findings, then **re-runs** `pat.regex.find_iter(&content)` independently when replacing content. Any filtering applied only in `scan_content()` is bypassed during actual mutation. Gaps 1 and 4 therefore require a shared eligible-match helper that all callsites use. This is a structural change, not a localized patch.

### Mutation paths in the current code

There are three *kinds* of path where pattern matches drive content changes, but the `redact()` function's replacement loop is duplicated across **thirteen** snapshot surfaces, and `redact_string()` is called from **four** additional surfaces. The complete inventory:

#### Path A: `redact_string()` — standalone string-in/string-out redaction

Iterates `PATTERNS`, runs `pat.regex.find_iter(&result)`, replaces matches end-to-start. Returns `Cow::Borrowed` when clean. Used by `redact()` for surfaces where the content is a single line or short string and a separate `scan_content()` call generates the findings:

| # | Surface | Field mutated | Line (approx) |
|---|---------|--------------|----------------|
| A1 | Container env vars | `container.env[*]` | 645 |
| A2 | SELinux audit rules | `rule.content` | 843 |
| A3 | SELinux PAM configs | `pam.content` | 851 |
| A4 | Sudoers rules | `users.sudoers_rules[*]` | 946 |

Each of these callers also calls `scan_content()` on the original (pre-redaction) content to generate findings. The mutation is done by `redact_string()`, not by the `scan_content` + re-scan pattern used elsewhere.

#### Path B: `scan_content()` — finding generation only

Iterates `PATTERNS`, runs `pat.regex.find_iter(content)`, pushes `RedactionFinding` structs. Does NOT mutate content. Currently applies **no filtering** — every regex match produces a finding.

#### Path C: `redact()` replacement loops — file-level redaction with re-scan

Calls `scan_content()` to get findings, then for each High-confidence finding, looks up the pattern by `kind_label` in `PATTERNS` and **re-runs** `pat.regex.find_iter(&content)` to collect matches for replacement. This loop is duplicated across thirteen surfaces:

| # | Surface | Section comment | Field mutated | Line (approx) |
|---|---------|----------------|--------------|----------------|
| C1 | Config files | `config.files` | `file.content` | 374 |
| C2 | RPM repo files | `rpm.repo_files` | `repo_file.content` | 411 |
| C3 | GPG keys | `rpm.gpg_keys` | `gpg_key.content` | 444 |
| C4 | Systemd drop-ins | `services.drop_ins` | `drop_in.content` | 479 |
| C5 | Kernel cmdline | `kernel_boot.cmdline` | `kernelboot.cmdline` | 561 |
| C6 | Dracut/modprobe/modules-load/tuned snippets | `kernel_boot.*_conf` | `snippet.content` | 598 |
| C7 | GeneratedTimerUnit commands | `sched.generated_timer_units` | `unit.command` | 663 |
| C8 | GeneratedTimerUnit service content | `sched.generated_timer_units` | `unit.service_content` | 695 |
| C9 | AtJob commands | `sched.at_jobs` | `at_job.command` | 730 |
| C10 | SystemdTimer exec_start | `sched.systemd_timers` | `timer.exec_start` | 768 |
| C11 | SystemdTimer service content | `sched.systemd_timers` | `timer.service_content` | 810 |
| C12 | .env file content | `non_rpm_software.env_files` | `env_file.content` | 866 |
| C13 | Git remote URLs (generic pattern pass) | `non_rpm_software.items` | `item.git_remote` | 913 |

**Note on git remotes (C13):** This surface also has dedicated `mask_proxy_credentials()` and `mask_token_username()` calls that use their own regexes (`PROXY_CRED_RE`, `TOKEN_USERNAME_RE`). These are structurally separate from the `PATTERNS`-based loop and do not need `collect_eligible_matches`. Only the generic `scan_content` + re-scan pass at C13 is in scope.

#### Surfaces NOT in scope

These use dedicated regexes (not `PATTERNS`) and are structurally different:

- **Proxy lines** (line 632): `mask_proxy_credentials()` with `PROXY_CRED_RE` / `PROXY_PASSWORD_KV_RE`
- **Git remote dedicated maskers** (lines 899, 905): `mask_proxy_credentials()` and `mask_token_username()` with `TOKEN_USERNAME_RE`
- **Fstab mount options** (line 518): `redact_mount_options()` with inline string parsing
- **Shadow entries** (line 955+): `scan_shadow()` / `classify_shadow_line()` — separate classifier, not pattern-based
- **Redaction hints** (line 1092+): Inspector-emitted hints, processed after all regex passes

### The bug

Paths A and C run the regex independently of path B. If `scan_content()` (path B) were to filter a match (e.g., skipping a comment line), the replacement paths (A and C) would still find and replace it because they re-scan fresh. The spec must fix this by making all paths use the same filtered match collection.

---

## Structural prerequisite: `collect_eligible_matches`

### Problem

Comment-line filtering (Gap 1) and false-positive value filtering (Gap 4) must apply to both finding generation AND content replacement. The current architecture has no shared filtering point — each callsite runs `pat.regex.find_iter()` independently.

### What to change

**File:** `inspectah-pipeline/src/redaction/engine.rs`

Introduce a `collect_eligible_matches` function that runs the regex, applies all filters, and returns the surviving matches. All mutation paths call this instead of raw `pat.regex.find_iter()`.

### How to implement

```rust
/// A match that survived all eligibility filters.
/// Contains byte offsets and the matched text, ready for replacement or finding generation.
struct EligibleMatch {
    start: usize,
    end: usize,
    text: String,
    line_num: usize,
}

/// Collect regex matches from `content` for a single `pat`, filtering out:
/// - Matches on comment lines (# // ;)
/// - Password-pattern matches whose value is a known NSS/PAM token
/// - PasswordHash matches when `path` is a shadow file (scan_shadow owns those)
///
/// This is the ONLY function that should call `pat.regex.find_iter()`.
/// Callers must invoke this for EVERY pattern, merge the results into a
/// single `Vec<(usize, EligibleMatch)>` (tagged with the pattern index),
/// then run `dedup_overlapping_matches` on the merged list before
/// processing matches. See the callsite changes section for the full flow.
fn collect_eligible_matches(
    pat: &SecretPattern,
    content: &str,
    path: Option<&str>,
) -> Vec<EligibleMatch> {
    pat.regex
        .find_iter(content)
        .filter_map(|mat| {
            let start = mat.start();
            let end = mat.end();
            let text = mat.as_str();

            // Filter 1: skip matches on comment lines
            if is_comment_line(content, start) {
                return None;
            }

            // Filter 2: skip Password matches whose value is a known
            // NSS/PAM false-positive token
            if pat.finding_kind == FindingKind::Password {
                if let Some(sep_pos) = text.find('=').or_else(|| text.find(':')) {
                    let value = &text[sep_pos + 1..];
                    if is_false_positive_value(value) {
                        return None;
                    }
                }
            }

            // Filter 3: skip PasswordHash matches when path is a shadow
            // file — scan_shadow owns those with richer classification
            if pat.finding_kind == FindingKind::PasswordHash {
                if let Some(p) = path {
                    if p.ends_with("/shadow") || p.ends_with("/shadow-") {
                        return None;
                    }
                }
            }

            let line_num = content[..start].lines().count() + 1;

            Some(EligibleMatch {
                start,
                end,
                text: text.to_string(),
                line_num,
            })
        })
        .collect()
}
```

### Callsite changes

The refactored callsites must collect matches from ALL patterns before dedup and replacement. This is the key architectural change: the current code iterates patterns one at a time, but cross-pattern dedup requires the merged match list from every pattern. The `collect_eligible_matches` helper runs per-pattern, but each callsite must call it for EVERY pattern, merge the results, run `dedup_overlapping_matches`, and only then process the surviving matches.

**`scan_content()` (Path B)** — replace the per-pattern inner loop with all-patterns collection + dedup:

```rust
// Before (current):
for pat in PATTERNS.iter() {
    let kind_label = pat.finding_kind.label();
    for mat in pat.regex.find_iter(content) {
        let line_num = content[..mat.start()].lines().count() + 1;
        findings.push(RedactionFinding { ... });
    }
}

// After:
let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
for (idx, pat) in PATTERNS.iter().enumerate() {
    for em in collect_eligible_matches(pat, content, path) {
        all_matches.push((idx, em));
    }
}
dedup_overlapping_matches(&mut all_matches);
for (pat_idx, em) in all_matches {
    let pat = &PATTERNS[pat_idx];
    findings.push(RedactionFinding {
        line: Some(em.line_num as i32),
        kind_label: pat.finding_kind.label().to_string(),
        // ... rest unchanged
    });
}
```

**`redact_string()` (Path A)** — replace the per-pattern loop with all-patterns collection + dedup:

```rust
// Before (current):
for pat in PATTERNS.iter() {
    let kind_label = pat.finding_kind.label();
    let matches: Vec<(usize, usize, String)> = pat
        .regex
        .find_iter(&result)
        .map(|m| (m.start(), m.end(), m.as_str().to_string()))
        .collect();
    for (start, end, matched) in matches.into_iter().rev() {
        let token = registry.token_for(&kind_label, &matched);
        buf.replace_range(start..end, &token);
    }
}

// After:
let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
for (idx, pat) in PATTERNS.iter().enumerate() {
    for em in collect_eligible_matches(pat, &result, None) {
        all_matches.push((idx, em));
    }
}
dedup_overlapping_matches(&mut all_matches);
// Sort descending by offset so replacements don't invalidate earlier offsets
all_matches.sort_by(|a, b| b.1.start.cmp(&a.1.start));
for (pat_idx, em) in all_matches {
    let kind_label = PATTERNS[pat_idx].finding_kind.label();
    let token = registry.token_for(kind_label, &em.text);
    buf.replace_range(em.start..em.end, &token);
}
```

**`redact()` replacement loops (Path C, all 13 sections C1-C13)** — replace the per-finding re-scan with all-patterns collection + dedup. This eliminates the "look up pattern by kind_label and re-scan" indirection entirely:

```rust
// Before (current, repeated in 13 sections):
// scan_content() generates findings, then for each finding:
let pat = PATTERNS.iter().find(|p| p.finding_kind.label() == kind_label).unwrap();
let matches: Vec<(usize, usize, String)> = pat
    .regex
    .find_iter(&content)
    .map(|m| (m.start(), m.end(), m.as_str().to_string()))
    .collect();
for (start, end, matched) in matches.into_iter().rev() {
    let token = registry.token_for(&kind_label, &matched);
    content.replace_range(start..end, &token);
}

// After (once per content blob, replacing the per-finding loop):
let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
for (idx, pat) in PATTERNS.iter().enumerate() {
    for em in collect_eligible_matches(pat, &content, Some(path)) {
        all_matches.push((idx, em));
    }
}
dedup_overlapping_matches(&mut all_matches);
// Sort descending by offset so replacements don't invalidate earlier offsets
all_matches.sort_by(|a, b| b.1.start.cmp(&a.1.start));
for (pat_idx, em) in all_matches {
    let kind_label = PATTERNS[pat_idx].finding_kind.label();
    let token = registry.token_for(kind_label, &em.text);
    content.replace_range(em.start..em.end, &token);
}
```

**Implementation note on Path A surfaces (A1-A4):** These currently use `redact_string()` for mutation and `scan_content()` for findings. Once both functions use the all-patterns + dedup flow, filtering and dedup are consistent. No structural change is needed to Path A surfaces — `redact_string()` itself is refactored as shown above, and its callers remain unchanged.

**Implementation note on Path C `scan_content` calls:** Each Path C section currently calls `scan_content()` to generate findings and then re-scans for replacement. With the refactored model, the replacement loop collects and deduplicates independently. The `scan_content()` call still generates findings (also using the all-patterns + dedup flow), but the replacement loop no longer depends on its output for pattern lookup — both use the same `collect_eligible_matches` + `dedup_overlapping_matches` pipeline.

### Risk/complexity

**Medium.** This is a structural refactor touching the core redaction path. The function itself is straightforward (filter iterator + collect), but it touches every mutation path in `engine.rs` — 13 Path C sections, the Path B inner loop, and the Path A `redact_string` function. The risk is introducing a regression in match offsets or missing a callsite. The dedup function introduces an intentional behavior change for existing Password/JdbcPassword overlaps (see Implementation order, step 1). Mitigation: the existing test suite covers all redaction surfaces, the new regression tests lock in the dedup behavior for known overlaps, and the gap-specific tests validate the filtering behavior end-to-end through `redact()`, not just through `scan_content()`.

**Refactoring opportunity:** The 13 Path C sections are copy-pasted boilerplate. A follow-up refactor could extract a `redact_content_inplace(content: &mut String, path: &str, registry: &mut CounterRegistry) -> Vec<RedactionFinding>` helper that encapsulates the `scan_content` + `collect_eligible_matches` + replace loop. This is out of scope for this spec but would reduce the surface area for future bugs. When the helper exists, the callsite inventory becomes irrelevant — there will be exactly one implementation.

---

## Gap 1: Comment-line filtering

### Problem

Go's `redactText` calls `isCommentLine(out, matchStart)` before processing every regex match. Lines whose trimmed prefix starts with `#`, `//`, or `;` are skipped. The Rust engine has no equivalent — all three mutation paths process every match regardless of whether it falls on a comment line.

### What to change

**File:** `inspectah-pipeline/src/redaction/engine.rs`

Add a helper function `is_comment_line`, called from `collect_eligible_matches` (see structural prerequisite above).

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

This is called inside `collect_eligible_matches`, not directly from any mutation path. See the structural prerequisite section.

### What to test

These tests must exercise the full `redact()` path (not just `scan_content` or `redact_string`) to validate that comment-line filtering applies to content mutation, not just finding generation.

| Test case | Input | Expected |
|---|---|---|
| Hash comment preserved (via `redact()`) | Config file: `# password=old_value\npassword=real` | Comment line untouched in output, `password=real` replaced with redaction token. Finding generated only for the second line. |
| Semicolon comment preserved | `; token=example\ntoken=secret` | `;` line untouched |
| C-style comment preserved | `// api_key=docs_example\napi_key=live` | `//` line untouched |
| Inline comment (not a comment line) | `password=secret # old was foo` | Line IS redacted (comment marker is mid-line, not at start) |
| Indented comment | `  # password=old` | Treated as comment (trimmed prefix starts with `#`) |
| First line is comment | `# secret=abc` (no preceding newline) | Treated as comment |
| Mixed file through `redact()` | Config file with `# password=old\npassword=real` run through full snapshot redaction | Only line 2 mutated; line 1 byte-identical to input |

### Risk/complexity

**Low.** The `is_comment_line` function is a string scan to the previous newline — no allocation, no regex. The only subtlety is ensuring `pos=0` (first line, no newline before it) works, which the `map_or(0, ...)` handles. Risk is fully contained in `collect_eligible_matches`.

---

## Gap 2: Inline PASSWORD_HASH pattern

### Problem

Go defines a standalone regex for crypt password hashes that fires on ANY content. This catches crypt hashes in htpasswd files, Kickstart configs, Ansible vault snippets, and anywhere else a `$6$salt$hash` appears.

Rust detects crypt hashes only through `classify_shadow_line()` in `patterns.rs`, which is scoped to `/etc/shadow` paths via the `scan_shadow` function. The `PATTERNS` vec has no `PasswordHash` entry. Any crypt hash outside a shadow file is invisible.

### Regex derivation (corrected from r1 review)

The original spec used Go's regex verbatim: `\$[1256y]\$[A-Za-z0-9./]+\$[A-Za-z0-9./]+`. This regex has three problems:

1. **`$6$rounds=5000$salt$hash` does not match.** The `=` in `rounds=5000` is not in `[A-Za-z0-9./]`, so the first character class stops at `rounds`, then requires `$` but finds `=`. The entire match fails.

2. **Yescrypt `$y$j9T$salt$hash` partially matches.** The regex captures `$y$j9T$salt` (two `$`-delimited segments) but drops `$hash` (the third segment). The hash output is left unredacted.

3. **bcrypt `$2b$12$...` does not match.** The algorithm ID `2b` is not in `[1256y]`.

The modular crypt format is: `$id[$param]$salt$hash` where:
- `id` is the algorithm identifier: `1` (MD5), `2a`/`2b`/`2y` (bcrypt), `5` (SHA-256), `6` (SHA-512), `y`/`gy` (yescrypt), `7` (scrypt), `sha1`
- `param` is optional (e.g., `rounds=5000` for SHA-256/512, `j9T` for yescrypt cost)
- `salt` and `hash` are `[A-Za-z0-9./+=]`

The corrected regex uses a repeating middle group to handle the variable number of `$`-delimited segments:

```
\$(?:1|2[aby]?|5|6|y|gy|7|sha1)\$(?:[A-Za-z0-9./+=]+\$){1,2}[A-Za-z0-9./+=]+
```

**How it works:**
- `\$(?:1|2[aby]?|5|6|y|gy|7|sha1)\$` — matches `$id$` with all known algorithm IDs
- `(?:[A-Za-z0-9./+=]+\$){1,2}` — matches 1 or 2 intermediate `segment$` blocks (covers `salt$` or `param$salt$`)
- `[A-Za-z0-9./+=]+` — matches the final hash output

**Verified against:**

| Input | Original regex | Corrected regex |
|---|---|---|
| `$6$rounds=5000$salt$hash` | NO MATCH | MATCH (full) |
| `$y$j9T$salt$hash` | Partial (`$y$j9T$salt`) | MATCH (full) |
| `$5$salt$hash` | MATCH | MATCH |
| `$1$abc$def` | MATCH | MATCH |
| `$2b$12$salt$hash` | NO MATCH | MATCH |
| `$HOME` | No match | No match |
| `FOO=$BAR` | No match | No match |
| `$1$a` | No match | No match (too few segments) |

### PasswordHash vs Password overlap (added in r3)

When `PasswordHash` is added to `PATTERNS`, the string `password=$6$rounds=5000$salt$hash` will match both:
- **`Password` pattern** (index 1): matches `password=$6$rounds=5000$salt$hash` (the whole assignment)
- **`PasswordHash` pattern** (new): matches `$6$rounds=5000$salt$hash` (the crypt hash portion)

These matches overlap — the `PasswordHash` span is a strict subset of the `Password` span. Both patterns fire independently because `scan_content` and the replacement loops iterate every pattern. Without a dedup rule, the replacement loops will attempt to replace both, causing double-redaction or corrupt offsets.

**Rule: longest-match-wins dedup at collection time.**

After all patterns have been collected, a dedup pass removes any match whose byte range `[start, end)` is entirely contained within another match's range. When two matches overlap, the longer one wins. When they are the same length (unlikely but possible), the one from the pattern earlier in the `PATTERNS` vec wins.

This dedup is applied:
- In `scan_content()`: after collecting all `EligibleMatch` results from all patterns, before pushing `RedactionFinding` structs.
- In replacement loops (Path C): after collecting all matches for a given content blob, before applying replacements.
- In `redact_string()` (Path A): same — dedup across all patterns before replacing.

**Implementation:**

```rust
/// Remove matches that are entirely contained within a longer match.
/// Input must be the combined matches from ALL patterns for a single content blob.
/// Preserves the longer match when spans overlap. Ties broken by input order
/// (earlier pattern in PATTERNS vec wins).
fn dedup_overlapping_matches(matches: &mut Vec<(usize, EligibleMatch)>) {
    // Sort by start ascending, then by span length descending (longer first)
    matches.sort_by(|a, b| {
        a.1.start.cmp(&b.1.start)
            .then_with(|| (b.1.end - b.1.start).cmp(&(a.1.end - a.1.start)))
    });
    let mut keep = vec![true; matches.len()];
    for i in 0..matches.len() {
        if !keep[i] { continue; }
        for j in (i + 1)..matches.len() {
            if !keep[j] { continue; }
            // If j is entirely within i's span, drop j
            if matches[j].1.start >= matches[i].1.start
                && matches[j].1.end <= matches[i].1.end
            {
                keep[j] = false;
            }
            // If j starts beyond i's end, no more overlaps possible
            if matches[j].1.start >= matches[i].1.end {
                break;
            }
        }
    }
    let mut idx = 0;
    matches.retain(|_| { let k = keep[idx]; idx += 1; k });
}
```

The tuple `(usize, EligibleMatch)` carries the pattern index for the tie-breaking guarantee. The function operates on the merged match list from all patterns, ensuring cross-pattern dedup. Each callsite (scan_content, redact_string, Path C loops) collects matches from all patterns first, calls `dedup_overlapping_matches`, then processes the surviving matches.

**Architectural note:** This changes the replacement loop structure. Currently, Path C sections iterate findings (from `scan_content`), look up the matching pattern, and re-scan with that single pattern. With the dedup rule, the loop must instead:
1. Call `collect_eligible_matches` for EVERY pattern, tagging each match with its pattern index and `kind_label`
2. Run `dedup_overlapping_matches` on the merged list
3. Sort by offset descending and replace

This eliminates the "look up pattern by kind_label and re-scan" indirection entirely — another simplification that falls out of the structural refactor.

### What to change

**File:** `inspectah-pipeline/src/redaction/patterns.rs`

Add a `PasswordHash` pattern entry to the `PATTERNS` vec with the corrected regex.

### How to implement

Add a new `SecretPattern` entry to the `PATTERNS` `LazyLock` vec. Place it BEFORE the generic `Password` entry so that in same-offset ties the more specific pattern wins, though the dedup rule handles cross-pattern overlap regardless of vec order.

```rust
// Modular crypt password hashes ($1$, $2b$, $5$, $6$, $y$, etc.)
// anywhere in content. Shadow files are handled separately by
// scan_shadow, but hashes appear in htpasswd, kickstart, ansible,
// and other configs.
SecretPattern {
    regex: Regex::new(
        r"\$(?:1|2[aby]?|5|6|y|gy|7|sha1)\$(?:[A-Za-z0-9./+=]+\$){1,2}[A-Za-z0-9./+=]+"
    ).unwrap(),
    finding_kind: FindingKind::PasswordHash,
    detection_method: DetectionMethod::Pattern,
    confidence: Confidence::High,
    remediation: "Remove password hash or use a secrets manager",
},
```

`FindingKind::PasswordHash` already exists in the enum. No type changes needed.

### What to test

| Test case | Input | Expected |
|---|---|---|
| SHA-512 with rounds | `admin:$6$rounds=5000$salt$longhash` | `PasswordHash` finding, hash redacted |
| yescrypt full match | `rootpw --iscrypted $y$j9T$salt$hash` | `PasswordHash` finding, entire `$y$...$hash` redacted |
| MD5 crypt hash | `$1$abc$def` | `PasswordHash` finding |
| SHA-256 crypt hash | `$5$salt$hash` | `PasswordHash` finding |
| bcrypt | `$2b$12$WApznUPhDubN0oeveSE3MOhash` | `PasswordHash` finding |
| Non-crypt dollar signs | `$HOME`, `$PATH` | No match |
| Shell assignment | `FOO=$BAR` | No match |
| Incomplete hash | `$1$a` | No match (too few segments) |
| **Overlap: password= with crypt hash** | `password=$6$rounds=5000$salt$hash` | Single finding: `Password` kind (longer match wins). The `PasswordHash` submatch is suppressed by dedup. Content is fully redacted — the `Password` replacement covers the entire `password=$6$...$hash` span. |
| **Standalone crypt hash (no key=)** | `$6$rounds=5000$salt$hash` (not preceded by `password=`) | `PasswordHash` finding only. No `Password` match because there is no key prefix. |

**Shadow overlap note:** With this pattern, crypt hashes in `/etc/shadow` will produce findings from both `scan_shadow` (which classifies locked/disabled/hash semantics) AND the generic pattern. Two options:

- **Option A (recommended):** Skip `PasswordHash` pattern matches when `path` is a shadow file. `collect_eligible_matches` already accepts `path: Option<&str>` — add a check: when the path is a shadow file AND the pattern is `PasswordHash`, skip the match. Keeps `scan_shadow`'s richer classification as the authority for shadow files.
- **Option B:** Accept dual findings. Simpler but noisier.

Recommend Option A. The `path` parameter is already part of the `collect_eligible_matches` signature (used by all callsites). The shadow check is a single `if` guard alongside the existing comment-line and false-positive filters.

### Risk/complexity

**Medium.** The pattern addition itself is low risk — single entry in an existing vec with a regex derived from the modular crypt format specification. The dedup rule adds implementation complexity: it changes the replacement loop structure from per-finding re-scan to all-patterns-then-dedup, which is a non-trivial refactor of all 13 Path C sections. However, this refactor is already required by the `collect_eligible_matches` prerequisite, so the marginal cost of adding dedup is small. The shadow overlap is handled by a path check in the helper.

---

## Gap 3: PEM block matching is partial

### Problem

Go's PEM patterns use `(?s)` (dot-matches-newline) to capture the entire `BEGIN...END` block including the base64-encoded key material between the markers.

Rust's pattern matches only the `BEGIN` header line:

```
-----BEGIN\s+(?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----
```

This means `redact_string` replaces only the header with a token, leaving the actual key material (the base64 lines and `END` marker) in the output. The finding is recorded, but the secret is not removed.

### What to change

**File:** `inspectah-pipeline/src/redaction/patterns.rs`

1. Replace the `PrivateKey` regex with a multi-line block pattern.
2. Add a `Certificate` block pattern (absent from Rust's `PATTERNS`).

### How to implement

Replace the existing `PrivateKey` entry:

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

Add a new `Certificate` entry:

```rust
SecretPattern {
    regex: Regex::new(
        r"(?s)-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----"
    ).unwrap(),
    finding_kind: FindingKind::Certificate,
    detection_method: DetectionMethod::Pattern,
    confidence: Confidence::High,
    remediation: "Review whether certificate exposure is intended; check for adjacent private keys",
},
```

`FindingKind::Certificate` already exists in the enum.

**Certificate redaction approach (clarified from r1 review):** The original spec stated that certificate blocks should "produce a finding but remain intact." This is inconsistent — every `SecretPattern` entry in `PATTERNS` is treated as an inline-replaceable secret by both `redact_string()` and the `redact()` replacement loops. There is no mechanism in the current architecture to produce a finding without redacting.

Two options were considered:

- **(a) Redact certificates like everything else.** This matches Go behavior, where PEM certificates WERE redacted. Simple, consistent with the existing architecture. Certificates are technically public data, but their presence in a config snapshot is worth flagging AND cleaning.
- **(b) Introduce a `redact: bool` field on `SecretPattern` or a `FindingAction` enum.** This would allow "flag but preserve" behavior. Requires changes to `SecretPattern`, `collect_eligible_matches`, all replacement loops, and test infrastructure.

**Decision: Option (a).** Certificates are redacted. The complexity of option (b) is not justified — certificates in config snapshots are noise that should be cleaned, and Go already redacted them. If a future use case needs flag-but-preserve semantics, that is a separate feature with its own spec.

### What to test

| Test case | Input | Expected |
|---|---|---|
| RSA private key block | Full `BEGIN RSA PRIVATE KEY` ... base64 ... `END RSA PRIVATE KEY` | Entire block replaced with single `REDACTED_PRIVATEKEY_1` token |
| EC private key block | `BEGIN EC PRIVATE KEY` ... `END EC PRIVATE KEY` | Entire block redacted |
| OPENSSH private key | `BEGIN OPENSSH PRIVATE KEY` ... `END OPENSSH PRIVATE KEY` | Entire block redacted |
| Certificate block | `BEGIN CERTIFICATE` ... `END CERTIFICATE` | Entire block redacted with `REDACTED_CERTIFICATE_1` token |
| Mixed PEM bundle | Certificate block + private key block | Both blocks redacted independently |
| Header-only (no END marker) | `-----BEGIN RSA PRIVATE KEY-----\ndata` (truncated, no END) | No match — avoids greedy consumption of unrelated content |
| Adjacent PEM blocks | Two private key blocks in sequence | Each matched independently (`.*?` is non-greedy) |

### Risk/complexity

**Low-medium.** The regex change is straightforward. `(?s)` is a supported inline flag in the `regex` crate. The non-greedy `.*?` is critical to prevent matching across unrelated blocks. The `redact_string` function's replace-from-end-to-start strategy handles multi-line replacements correctly since it operates on byte offsets. No structural changes needed to the replacement loop.

---

## Gap 4: Value-level false-positive filtering

### Problem

Go checks matched PASSWORD values against a set of 20 known NSS/PAM tokens before redacting. This fires for ANY path. When a match has `typeLabel == "PASSWORD"` and the captured value is in this set, the match is skipped.

Rust has a path-based allowlist (`REDACTION_ALLOWLIST` / `is_allowlisted_path()`), but it operates in a **completely different part of the code** than the pattern matching pipeline.

### Current `REDACTION_ALLOWLIST` behavior (corrected in r3)

The r2 spec incorrectly stated that `REDACTION_ALLOWLIST` "currently lives in `scan_content`" and "filters before the pattern loop." This is wrong. Here is what the code actually does:

- `REDACTION_ALLOWLIST` is defined at line 19: `&["etc/pam.d/"]`
- `is_allowlisted_path()` is defined at line 23
- `is_allowlisted_path()` is called at **exactly one location** — line 1095, inside the **redaction hints processing loop** (not in `scan_content`, not in any `PATTERNS` replacement loop)
- Its effect: when processing `snapshot.redaction_hints`, hints whose path matches the allowlist are silently skipped — no finding, no unresolved count, no impact on redaction state

This means `REDACTION_ALLOWLIST` has **no effect on `PATTERNS`-based scanning or redaction at all**. If a config file at `/etc/pam.d/system-auth` is included in `snapshot.config.files`, it WILL be scanned by `scan_content()` and its password-looking lines WILL be redacted. The allowlist only suppresses inspector-emitted hints for that path.

Practically, this distinction matters less than it appears: PAM files are typically not included in `config.files` by the inspector. But the spec must describe the actual behavior, not the assumed behavior.

### What actually needs to change

The false-positive problem is real — `passwd: files sss` in `/etc/nsswitch.conf` triggers the `Password` pattern via the `:` separator, even though the values are NSS module tokens, not secrets. Similarly, any config using `password=` or `password:` with a PAM/NSS token as the value (e.g., `password=files`, `password: sufficient`) would trigger a false positive. Note that PAM module stack lines like `password sufficient pam_unix.so` do NOT trigger the regex — they use whitespace, not `=` or `:` — but the `key=value` and `key: value` forms do appear in NSS and other configuration formats. The fix is a **value-level filter** inside `collect_eligible_matches`, as described in r2. This is new behavior, not a relocation of existing behavior.

**File:** `inspectah-pipeline/src/redaction/engine.rs`

Add a value-based false-positive check inside `collect_eligible_matches` (see structural prerequisite).

### How to implement

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

This is called inside `collect_eligible_matches` (see structural prerequisite), not directly from any mutation path. The filter applies to `Password`-kind patterns only, checking the value portion after `=` or `:`.

**`REDACTION_ALLOWLIST` disposition:** Keep `REDACTION_ALLOWLIST` and `is_allowlisted_path()` as-is in the hints processing loop. They serve a different purpose (suppressing inspector hints for known-noisy paths) and are not part of the `PATTERNS` pipeline. No changes needed.

### What to test

These tests must exercise the full `redact()` path to validate that false-positive filtering applies to content mutation, not just finding generation.

| Test case | Input (path, content) | Expected |
|---|---|---|
| nsswitch.conf with NSS tokens | `/etc/nsswitch.conf`, `passwd: files sss` | No PASSWORD finding, no redaction. `passwd:` triggers regex (`:` separator), but value `files` is a false positive. |
| NSS token via equals | `/etc/nsswitch.conf`, `password=files` | No finding. `password=` triggers the regex, but value `files` is a false positive. |
| PAM token via colon separator | `/etc/pam.d/system-auth`, `password: sufficient` | No finding. `password:` triggers the regex (`:`separator), but value `sufficient` is a known false positive. |
| PAM module via equals | `/etc/security/custom.conf`, `password=pam_unix.so` | No finding. `password=` triggers the regex, but value `pam_unix.so` is a known false positive. |
| Real password after key | `/etc/app.conf`, `password=s3cret` | PASSWORD finding, value redacted. |
| Mixed line via `redact()` | Config file: `passwd: files\ndb_password=real` | First line untouched in output (value `files` is false positive). Second line redacted. |
| Case insensitivity | `password: FILES` | No finding (case-insensitive check). |
| Real secret in any path | `/etc/pam.d/custom`, `password=actualpass123` | Redacted (value `actualpass123` not in false-positive set). |

**Note on PAM config lines without separators (corrected in r4):** Lines like `password requisite pam_pwquality.so` and `password sufficient pam_unix.so` (PAM module stack format) never match the `Password` regex because the regex requires `=` or `:` after the keyword — these lines use whitespace separation, not key-value syntax. Earlier revisions included test rows for these inputs, but they were vacuous: the test outcome ("no finding") was correct, but the false-positive filter never executed because the regex itself prevented the match. The corrected test rows above use inputs with `=` or `:` separators that DO trigger the regex, ensuring the false-positive value filter is the mechanism being tested.

### Risk/complexity

**Low-medium.** The value extraction depends on the PASSWORD regex's structure — it requires `=` or `:` as a separator. The current Rust regex `(?i)(?:password|passwd|...)\\s*[=:]\\s*\\S+` guarantees this, meaning the filter only activates on lines that actually use key-value syntax. PAM module stack lines (`password sufficient pam_unix.so`) never reach the filter because the regex itself rejects them — there is no `=` or `:` separator. If the regex changes in the future, the separator extraction must stay in sync. A comment in the code should note this coupling.

---

## Implementation order

1. **Structural prerequisite (`collect_eligible_matches` + `dedup_overlapping_matches`)** — must land first. Introduces the shared helper with empty filter bodies plus the overlap dedup function. All mutation paths (`scan_content`, `redact_string`, and all 13 `redact()` Path C sections) switch to using it.

   **Behavior change (corrected in r4):** This step is NOT behavior-neutral. The dedup function applies globally to all patterns, and existing patterns already overlap — specifically, `Password` and `JdbcPassword` overlap on JDBC connection strings (e.g., `jdbc:postgresql://host/db?password=s3cret`). The `Password` pattern matches `password=s3cret` (a strict subset), while `JdbcPassword` matches the entire URL. With dedup, the shorter `Password` match is suppressed in favor of the longer `JdbcPassword` match.

   **Why global dedup is correct:** Scoping dedup to only PasswordHash-involved overlaps would couple the dedup logic to specific pattern identities — the wrong abstraction. Dedup is a geometric operation on byte ranges that should be pattern-agnostic. The existing Password/JdbcPassword overlap is a pre-existing inaccuracy: reporting `Password` for a JDBC connection string is less informative than reporting `JdbcPassword`. The dedup corrects this.

   **Impact assessment:** The behavior change is benign. The redaction *outcome* is identical — the credential is fully covered by the longer `JdbcPassword` span regardless. Only the reported `FindingKind` changes from `Password` to `JdbcPassword`, which is more accurate. No other existing pattern pairs overlap (PostgresPassword and MongodbPassword use URI syntax without `key=value`, so the `Password` pattern does not match them).

   **Regression tests required:** Add test cases for the existing Password/JdbcPassword overlap to lock in the new behavior:

   | Test case | Input | Before dedup | After dedup |
   |---|---|---|---|
   | JDBC URL with password param | `jdbc:postgresql://host:5432/db?user=admin&password=s3cret` | Two findings: `Password` for `password=s3cret` + `JdbcPassword` for entire URL | Single finding: `JdbcPassword` (longer match wins) |
   | JDBC URL, value fully redacted | Same input through `redact()` | Redacted (both patterns fire, replacement outcome correct by accident) | Redacted (single clean replacement) |
   | Password without JDBC context | `password=s3cret` (standalone) | `Password` finding | `Password` finding (no overlap, unchanged) |
   | JDBC URL without password param | `jdbc:postgresql://host:5432/db?user=admin` | No `Password` finding, no `JdbcPassword` finding | Unchanged |

   Existing tests must still pass — the dedup does not remove any matches that aren't already subsumed by a longer match, so no previously-redacted content becomes un-redacted.

2. **Gap 1 (comment filtering)** — add `is_comment_line` and wire it into `collect_eligible_matches`. Tests validate filtering through `redact()`, not just `scan_content`.

3. **Gap 4 (false-positive values)** — add `FALSE_POSITIVE_VALUES` and the value check into `collect_eligible_matches`. Depends on Gap 1 being in place (both are filters in the same function). `REDACTION_ALLOWLIST` remains untouched.

4. **Gap 2 (inline PASSWORD_HASH + overlap dedup)** — pattern addition with corrected regex. Benefits from Gaps 1 and 4 filtering automatically via `collect_eligible_matches`. Add path-based shadow exclusion. The dedup function (already landed in step 1) handles the Password/PasswordHash overlap. Add the overlap test cases.

5. **Gap 3 (PEM block matching)** — regex replacement + certificate pattern addition. No dependencies on other gaps but test last since it changes match spans from single-line to multi-line.

## Estimated effort

The structural prerequisite adds a refactoring step the original spec did not include, and the callsite count is significantly larger than originally estimated. Revised estimate:

- **Structural prerequisite:** ~120 lines of production code (helper + dedup function + 13 Path C callsite changes + `scan_content` inner loop + `redact_string` inner loop), plus ~30 lines of regression tests for the existing Password/JdbcPassword overlap. Intentional behavior change (dedup reclassifies JDBC overlap findings from `Password` to `JdbcPassword`). Touches the critical path — careful testing required.
- **Gaps 1-4:** ~120 lines of production code (filters + patterns + regex), ~500 lines of tests (including overlap and dedup test cases).
- **Total:** ~240 lines production, ~500 lines tests. Implementable in a single session but should be committed incrementally per the implementation order above.

## Complete callsite reference

For implementer convenience, here is every location in `engine.rs` that currently calls `pat.regex.find_iter()` or `pat.regex.is_match()` and must be converted. The conversion is NOT a mechanical 1:1 replacement of each `find_iter` call — instead, each *function* (`scan_content`, `redact_string`, and each Path C section) replaces its entire per-pattern loop with the all-patterns + dedup flow shown in the callsite changes section above.

| Function | Line (approx) | Current call | Conversion |
|----------|---------------|-------------|------------|
| `redact_string` | 117 | `pat.regex.is_match(content)` | Keep as fast-path pre-check (see note below) |
| `redact_string` | 136 | `pat.regex.find_iter(&result)` | Replaced by all-patterns loop: `collect_eligible_matches(pat, &result, None)` for each pattern, then `dedup_overlapping_matches` on the merged list |
| `scan_content` | 256 | `pat.regex.find_iter(content)` | Replaced by all-patterns loop: `collect_eligible_matches(pat, content, path)` for each pattern, then `dedup_overlapping_matches` on the merged list |
| `redact()` C1-C13 | various | `pat.regex.find_iter(&content)` | Each section's per-finding re-scan loop replaced by all-patterns loop: `collect_eligible_matches(pat, &content, Some(path))` for each pattern, then `dedup_overlapping_matches` on the merged list |

**Note on `redact_string` is_match (line 117):** This is a fast-path optimization — it short-circuits before cloning the string. It does NOT need filtering (a false positive that passes `is_match` but fails `collect_eligible_matches` just means a wasted clone, not incorrect output). Keep it as-is or convert — implementer's discretion.

**Key difference from per-pattern conversion:** The table above lists individual `find_iter` call sites for locating them in the source. But the conversion is structural — you do not replace each `find_iter` individually. You replace the enclosing per-pattern loop with a single all-patterns collection pass, as shown in the Before/After snippets in the callsite changes section. The individual `find_iter` calls disappear as a side effect of that restructuring.
