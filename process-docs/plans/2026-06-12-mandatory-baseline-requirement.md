# Mandatory Baseline Requirement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the `--no-baseline` code path from inspectah, making baseline extraction a hard requirement with classified error messages on failure.

**Architecture:** Remove the `no_baseline` bool from snapshot and RPM types, delete the `--no-baseline` CLI flag, convert baseline pull failure into a hard error with a five-category classifier and formatted remediation output. Bump schema version 18 → 19.

**Tech Stack:** Rust (clap, serde, thiserror), inspectah workspace crates (cli, core, collect, pipeline, refine, web), TypeScript (web UI)

**Spec:** `process-docs/specs/proposed/2026-06-12-mandatory-baseline-requirement.md`

**Task ordering rationale:** Every task leaves `cargo test --workspace` green. The schema version bump lands last (after all `no_baseline` code is removed) so old tests never reference deleted fields. The stderr credential audit lands before the first user-visible classified error output. Frontend and docs land after Rust is clean.

**Preservation note:** `sysctl_no_baseline` in the frontend (`attentionUtils.ts`, `SysctlSection.test.tsx`, `api/types.ts`) is a per-inspector triage concept unrelated to the global `--no-baseline` flag. Do NOT remove it.

---

### Task 1: Add pull failure classifier, formatter, and credential sanitizer (cli crate)

**Owner:** Rust
**Files:**
- Create: `crates/cli/src/commands/pull_failure.rs`
- Modify: `crates/cli/src/commands/mod.rs`

This task adds the classifier, formatter, and sanitization as a self-contained module, and wires sanitization into the live pull progress path. No behavioral change to scan flow yet — the classifier/formatter aren't called until Task 3.

- [ ] **Step 1: Create `pull_failure.rs`**

```rust
// crates/cli/src/commands/pull_failure.rs

/// Categories of baseline pull failure, ordered by classifier priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullFailureKind {
    TlsCertError,
    AuthRequired,
    RegistryUnreachable,
    ImageNotFound,
    Unknown,
}

/// Classify a podman pull failure from its stderr output.
///
/// Priority order (first match wins): TLS > Auth > Registry > NotFound > Unknown.
pub fn classify_pull_failure(stderr: &str) -> PullFailureKind {
    let lower = stderr.to_lowercase();

    if lower.contains("certificate")
        || lower.contains("x509")
        || lower.contains("tls")
        || lower.contains("insecure")
    {
        return PullFailureKind::TlsCertError;
    }

    if lower.contains("unauthorized")
        || lower.contains("authentication required")
        || lower.contains("403")
        || lower.contains("login")
    {
        return PullFailureKind::AuthRequired;
    }

    if lower.contains("dial tcp")
        || lower.contains("no such host")
        || lower.contains("connection refused")
        || lower.contains("i/o timeout")
        || lower.contains("network is unreachable")
    {
        return PullFailureKind::RegistryUnreachable;
    }

    if lower.contains("manifest unknown")
        || lower.contains("404")
        || lower.contains("not found")
        || lower.contains("name unknown")
    {
        return PullFailureKind::ImageNotFound;
    }

    PullFailureKind::Unknown
}

/// Redact recognized credential patterns from stderr.
pub fn sanitize_stderr(s: &str) -> String {
    let mut result = s.to_string();
    // Case-insensitive pattern replacement without regex dependency.
    // Scan for known credential prefixes and redact the token that follows.
    let patterns = ["bearer ", "basic ", "authorization: "];
    let lower = result.to_lowercase();
    for pat in &patterns {
        if let Some(start) = lower.find(pat) {
            let token_start = start + pat.len();
            let token_end = result[token_start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .map(|i| token_start + i)
                .unwrap_or(result.len());
            if token_end > token_start {
                result.replace_range(token_start..token_end, "[REDACTED]");
            }
        }
    }
    result
}

fn registry_from_ref(image_ref: &str) -> &str {
    image_ref.split('/').next().unwrap_or(image_ref)
}

fn truncate_stderr(stderr: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(max_lines)
        .collect();
    lines.join("\n")
}

/// Format a pull failure into a user-facing error message with remediation.
pub fn format_pull_error(kind: PullFailureKind, image_ref: &str, raw_stderr: &str) -> String {
    let registry = registry_from_ref(image_ref);
    let mut out = String::new();

    out.push_str("Error: cannot pull baseline image\n\n");
    out.push_str(&format!("  Image:  {image_ref}\n"));

    match kind {
        PullFailureKind::RegistryUnreachable => {
            out.push_str(&format!("  Cause:  cannot reach registry ({registry})\n\n"));
            out.push_str("  Check network connectivity to the registry:\n");
            out.push_str(&format!(
                "    curl -s https://{registry}/v2/ || echo \"unreachable\"\n"
            ));
            out.push_str("  If behind a proxy, configure podman:\n");
            out.push_str(
                "    Edit /etc/containers/registries.conf or set HTTP_PROXY/HTTPS_PROXY\n",
            );
        }
        PullFailureKind::AuthRequired => {
            out.push_str("  Cause:  authentication required\n\n");
            out.push_str("  Verify the image reference is correct (a wrong registry can look like an auth error):\n");
            out.push_str(
                "    inspectah scan --base-image <correct-registry>/<image>:<tag>\n",
            );
            out.push_str("  If the reference is correct, log in to the registry:\n");
            out.push_str(&format!("    podman login {registry}\n"));
            out.push_str(
                "  For Red Hat registries, use your Red Hat account or a service account token.\n",
            );
        }
        PullFailureKind::ImageNotFound => {
            out.push_str("  Cause:  image or tag not found\n\n");
            out.push_str("  Verify the image reference is correct:\n");
            out.push_str(&format!("    podman search {image_ref}\n"));
            out.push_str(&format!(
                "    skopeo list-tags docker://{image_ref}\n"
            ));
            out.push_str("  If your image is at a different registry or tag, use:\n");
            out.push_str(
                "    inspectah scan --base-image <correct-registry>/<image>:<tag>\n",
            );
        }
        PullFailureKind::TlsCertError => {
            out.push_str("  Cause:  TLS certificate error\n\n");
            out.push_str("  Verify the image reference is correct (a wrong registry can cause TLS errors):\n");
            out.push_str(
                "    inspectah scan --base-image <correct-registry>/<image>:<tag>\n",
            );
            out.push_str("  If using a private registry with self-signed certificates:\n");
            out.push_str(
                "    sudo cp ca.crt /etc/pki/ca-trust/source/anchors/ && sudo update-ca-trust\n",
            );
            out.push_str("  Or configure podman to trust the registry:\n");
            out.push_str(
                "    Edit /etc/containers/registries.conf.d/ to add [[registry]] with insecure=true\n",
            );
        }
        PullFailureKind::Unknown => {
            out.push_str("  Cause:  pull failed\n\n");
            let sanitized = sanitize_stderr(raw_stderr);
            let excerpt = truncate_stderr(&sanitized, 3);
            if !excerpt.is_empty() {
                out.push_str("  podman reported:\n");
                for line in excerpt.lines() {
                    out.push_str(&format!("    {line}\n"));
                }
                out.push('\n');
            }
            out.push_str("  Try pulling the image manually to diagnose:\n");
            out.push_str(&format!("    podman pull {image_ref}\n"));
        }
    }

    out.push_str(&format!(
        "\n  Disconnected? You can load images from a tarball:\n"
    ));
    out.push_str(&format!("    podman save -o baseline.tar {image_ref}\n"));
    out.push_str("    podman load -i baseline.tar\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Classifier tests ---

    #[test]
    fn classify_tls() {
        assert_eq!(
            classify_pull_failure("x509: certificate signed by unknown authority"),
            PullFailureKind::TlsCertError
        );
    }

    #[test]
    fn classify_auth() {
        assert_eq!(
            classify_pull_failure("unauthorized: authentication required"),
            PullFailureKind::AuthRequired
        );
    }

    #[test]
    fn classify_unreachable() {
        assert_eq!(
            classify_pull_failure("dial tcp 10.0.0.1:443: i/o timeout"),
            PullFailureKind::RegistryUnreachable
        );
    }

    #[test]
    fn classify_not_found() {
        assert_eq!(
            classify_pull_failure("manifest unknown: manifest unknown"),
            PullFailureKind::ImageNotFound
        );
    }

    #[test]
    fn classify_unknown() {
        assert_eq!(
            classify_pull_failure("something completely unexpected"),
            PullFailureKind::Unknown
        );
    }

    #[test]
    fn classify_empty() {
        assert_eq!(classify_pull_failure(""), PullFailureKind::Unknown);
    }

    #[test]
    fn classify_priority_tls_over_auth() {
        assert_eq!(
            classify_pull_failure("unauthorized: x509 certificate error"),
            PullFailureKind::TlsCertError
        );
    }

    #[test]
    fn classify_priority_auth_over_unreachable() {
        assert_eq!(
            classify_pull_failure("connection refused, unauthorized access"),
            PullFailureKind::AuthRequired
        );
    }

    // --- Sanitizer tests ---

    #[test]
    fn sanitize_redacts_bearer_token() {
        let input = "Error: Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9 unauthorized";
        let result = sanitize_stderr(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("eyJhbG"));
    }

    #[test]
    fn sanitize_redacts_basic_auth() {
        let input = "Basic dXNlcjpwYXNz in header";
        let result = sanitize_stderr(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("dXNlcjpwYXNz"));
    }

    #[test]
    fn sanitize_redacts_authorization_header() {
        let input = "Authorization: token_abc123 sent";
        let result = sanitize_stderr(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("token_abc123"));
    }

    #[test]
    fn sanitize_preserves_non_credential_text() {
        let input = "manifest unknown: not found";
        assert_eq!(sanitize_stderr(input), input);
    }

    // --- Formatter tests ---

    #[test]
    fn format_includes_image_ref() {
        let msg = format_pull_error(
            PullFailureKind::AuthRequired,
            "quay.io/centos-bootc/centos-bootc:stream10",
            "",
        );
        assert!(msg.contains("quay.io/centos-bootc/centos-bootc:stream10"));
        assert!(msg.contains("cannot pull baseline image"));
    }

    #[test]
    fn format_includes_disconnected_hint() {
        let msg = format_pull_error(
            PullFailureKind::RegistryUnreachable,
            "quay.io/test:latest",
            "",
        );
        assert!(msg.contains("podman save"));
        assert!(msg.contains("podman load"));
    }

    #[test]
    fn format_unknown_includes_sanitized_stderr() {
        let stderr = "Bearer eyJsecret in line one\nline two\nline three\nline four";
        let msg = format_pull_error(PullFailureKind::Unknown, "quay.io/test:latest", stderr);
        assert!(msg.contains("[REDACTED]"));
        assert!(!msg.contains("eyJsecret"));
        assert!(msg.contains("line three"));
        assert!(!msg.contains("line four"));
    }

    #[test]
    fn format_auth_leads_with_base_image() {
        let msg = format_pull_error(
            PullFailureKind::AuthRequired,
            "registry.example.com/img:v1",
            "",
        );
        let base_image_pos = msg.find("--base-image").unwrap();
        let login_pos = msg.find("podman login").unwrap();
        assert!(
            base_image_pos < login_pos,
            "--base-image hint must appear before podman login"
        );
    }

    #[test]
    fn format_tls_leads_with_base_image() {
        let msg = format_pull_error(
            PullFailureKind::TlsCertError,
            "registry.example.com/img:v1",
            "",
        );
        let base_image_pos = msg.find("--base-image").unwrap();
        let ca_pos = msg.find("ca-trust").unwrap();
        assert!(
            base_image_pos < ca_pos,
            "--base-image hint must appear before CA trust instructions"
        );
    }

    #[test]
    fn truncate_clips_at_3_lines() {
        assert_eq!(
            truncate_stderr("one\ntwo\nthree\nfour\nfive", 3),
            "one\ntwo\nthree"
        );
    }

    #[test]
    fn truncate_skips_blank_lines() {
        assert_eq!(
            truncate_stderr("one\n\n\ntwo\n\nthree\nfour", 3),
            "one\ntwo\nthree"
        );
    }
}
```

- [ ] **Step 2: Add module to `commands/mod.rs`**

Add `pub mod pull_failure;` to `crates/cli/src/commands/mod.rs`.

- [ ] **Step 3: Add testable seam to live pull progress callbacks**

The callbacks currently write directly to `stderr` via `eprintln!` / `write!`. To prove sanitization at the display boundary, refactor to accept a `&mut dyn Write` output sink. Production passes `std::io::stderr()`, tests pass a `Vec<u8>` buffer.

Modify `crates/cli/src/commands/pull_progress.rs`:

For `non_tty_callback`, add an output parameter:
```rust
pub fn non_tty_callback<'a>(
    collected: &'a mut Vec<String>,
    output: &'a mut dyn std::io::Write,
) -> impl FnMut(&str) + 'a {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if !cleaned.trim().is_empty() {
            let safe = super::pull_failure::sanitize_stderr(&cleaned);
            let _ = writeln!(output, "  pull: {safe}");
        }
        collected.push(cleaned);
    }
}
```

For `tty_viewport_callback`, add an output parameter:
```rust
pub fn tty_viewport_callback<'a>(
    collected: &'a mut Vec<String>,
    ring: &'a mut [String],
    ring_pos: &'a mut usize,
    content_width: usize,
    output: &'a mut dyn std::io::Write,
) -> impl FnMut(&str) + 'a {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if cleaned.trim().is_empty() {
            return;
        }
        let safe = super::pull_failure::sanitize_stderr(&cleaned);
        collected.push(cleaned);

        let viewport_lines = ring.len();
        ring[*ring_pos % viewport_lines] = truncate_line(&safe, content_width);
        // ... rest of viewport rendering writes to `output` instead of stderr ...
```

Update the call sites in `scan.rs` to pass `&mut std::io::stderr()` (or `std::io::stderr().lock()`) — this is a mechanical change, no behavior change in production.

Note: `collected` still stores the raw (ANSI-stripped but unsanitized) line for post-pull blob counting and classification. Sanitization applies only to the *displayed* output written to the sink.

- [ ] **Step 4: Add tests proving live callbacks sanitize displayed output**

Add to `pull_progress.rs` tests:
```rust
#[test]
fn non_tty_callback_redacts_bearer_in_displayed_output() {
    let mut collected = Vec::new();
    let mut display_buf: Vec<u8> = Vec::new();
    {
        let mut cb = non_tty_callback(&mut collected, &mut display_buf);
        cb("Trying to pull: Bearer eyJsecret123 from registry");
    }
    let displayed = String::from_utf8(display_buf).unwrap();

    // Displayed output must be sanitized
    assert!(displayed.contains("[REDACTED]"), "displayed output must redact token");
    assert!(!displayed.contains("eyJsecret123"), "raw token must not appear in display");
    assert!(displayed.contains("pull:"), "prefix must be present");

    // Collected raw line is unsanitized (for classification)
    assert_eq!(collected.len(), 1);
    assert!(collected[0].contains("eyJsecret123"), "collected line preserves raw for classification");
}

#[test]
fn non_tty_callback_passes_clean_lines_unchanged() {
    let mut collected = Vec::new();
    let mut display_buf: Vec<u8> = Vec::new();
    {
        let mut cb = non_tty_callback(&mut collected, &mut display_buf);
        cb("Copying blob sha256:abc123 done");
    }
    let displayed = String::from_utf8(display_buf).unwrap();
    assert!(displayed.contains("Copying blob sha256:abc123 done"));
    assert!(!displayed.contains("[REDACTED]"));
}

#[test]
fn tty_viewport_callback_redacts_bearer_in_ring_buffer() {
    let mut collected = Vec::new();
    let mut ring = vec![String::new(); 4];
    let mut ring_pos = 0usize;
    let mut display_buf: Vec<u8> = Vec::new();
    {
        let mut cb = tty_viewport_callback(
            &mut collected,
            &mut ring,
            &mut ring_pos,
            60,
            &mut display_buf,
        );
        cb("Error: Bearer eyJtoken456 unauthorized");
    }
    // Ring buffer (what gets rendered in viewport) must be sanitized
    let ring_content = ring.join(" ");
    assert!(ring_content.contains("[REDACTED]"), "viewport ring must redact token");
    assert!(!ring_content.contains("eyJtoken456"), "raw token must not appear in viewport");

    // Collected line preserves raw for classification
    assert!(collected[0].contains("eyJtoken456"));
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-cli -- pull_failure`
Run: `cargo test -p inspectah-cli -- pull_progress`
Expected: all tests pass.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p inspectah-cli -- -D warnings`
Expected: zero warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/cli/src/commands/pull_failure.rs crates/cli/src/commands/mod.rs crates/cli/src/commands/pull_progress.rs
git commit -m "feat(cli): add pull failure classifier, formatter, and sanitizer

Adds PullFailureKind enum with five categories classified by
pattern-matching podman stderr. Deterministic priority order
(TLS > Auth > Registry > NotFound > Unknown). Includes
credential sanitization for bearer/basic/authorization
patterns in both live pull progress callbacks and final error
output. format_pull_error produces structured remediation
with disconnected workaround on every category.

Part of mandatory baseline requirement."
```

---

### Task 2: Remove `no_baseline` from all Rust code (atomic, compiler-clean)

**Owner:** Rust
**Files:**
- Modify: `crates/core/src/snapshot.rs` — remove `pub no_baseline: bool` field
- Modify: `crates/core/src/types/rpm.rs` — remove `pub no_baseline: bool` field
- Modify: `crates/collect/src/inspectors/rpm/mod.rs` — remove derivation + warning
- Modify: `crates/pipeline/src/render/baseline_fmt.rs` — remove `snap.no_baseline` branch
- Modify: `crates/pipeline/src/render/readme.rs` — remove `no_baseline` branch
- Modify: `crates/pipeline/src/render/service_intent.rs` — simplify compound condition
- Modify: `crates/pipeline/src/render/report.rs` — remove no-baseline panel test
- Modify: `crates/pipeline/src/render/containerfile.rs` — remove no-baseline test assertions
- Modify: `crates/core/src/fleet/merge.rs` — remove `no_baseline` merge logic
- Modify: `crates/core/src/fleet/mod.rs` — remove `merged.no_baseline` assignment
- Modify: `crates/refine/src/projection/types.rs` — remove `EmptyReason::NoBaseline`
- Modify: `crates/refine/src/projection/reference.rs` — remove NoBaseline logic + test
- Modify: `crates/refine/src/session.rs` — simplify `baseline_available`
- Modify: `crates/web/src/adapter.rs` — remove NoBaseline match arm
- Modify: `crates/cli/src/commands/scan.rs` — remove `snapshot.no_baseline` assignment (but keep the `--no-baseline` flag removal for Task 3)
- Modify: All test files with `no_baseline` in struct literals

This is one atomic commit. All `no_baseline` references are removed together so the compiler stays clean at every step.

- [ ] **Step 1: Remove `no_baseline` from `InspectionSnapshot`**

In `crates/core/src/snapshot.rs`, delete:

```rust
    /// True if baseline resolution was attempted but failed or is unavailable.
    /// Distinguishes "no baseline" from "baseline not yet attempted".
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub no_baseline: bool,
```

- [ ] **Step 2: Remove `no_baseline` from `RpmSection`**

In `crates/core/src/types/rpm.rs`, delete line 227:

```rust
    pub no_baseline: bool,
```

- [ ] **Step 3: Remove derivation in RPM inspector**

In `crates/collect/src/inspectors/rpm/mod.rs`, remove lines ~418-422:

```rust
        let no_baseline = ctx.baseline_data.is_none();
        if no_baseline {
            warnings.push(Warning {
                inspector: "rpm".into(),
                message: "no baseline available — all packages classified as added".into(),
            });
        }
```

And remove `no_baseline,` from the `RpmSection` struct literal (line ~447).

- [ ] **Step 4: Clean up renderers**

In `crates/pipeline/src/render/baseline_fmt.rs`: remove the `if snap.no_baseline` branch (line ~158-159).

In `crates/pipeline/src/render/readme.rs`: remove the `no_baseline` local and `if no_baseline` branch (lines ~100-102).

In `crates/pipeline/src/render/service_intent.rs`: simplify line ~317 from:
```rust
        rpm.no_baseline || rpm.baseline_package_names.is_none() || snap.no_baseline;
```
to:
```rust
        rpm.baseline_package_names.is_none();
```

- [ ] **Step 5: Clean up fleet merge**

In `crates/core/src/fleet/merge.rs`: remove `no_baseline` from the baseline host extraction (lines ~1036, 1043) and the struct literal (line ~1213).

In `crates/core/src/fleet/mod.rs`: delete line ~123:
```rust
    merged.no_baseline = merged.baseline.is_none();
```

- [ ] **Step 6: Clean up refine + web**

In `crates/refine/src/projection/types.rs`: remove `NoBaseline` from `EmptyReason`:
```rust
pub enum EmptyReason {
    ZeroDrift,
    DataUnavailable,
}
```

In `crates/refine/src/projection/reference.rs`: change the empty version_changes logic (lines ~36-41) from:
```rust
        let reason = if snap.baseline.is_some() {
            EmptyReason::ZeroDrift
        } else {
            EmptyReason::NoBaseline
        };
```
to:
```rust
        let reason = EmptyReason::ZeroDrift;
```

In `crates/refine/src/session.rs`: leave `baseline_available` derivation as-is (it checks `rpm.baseline_package_names.is_some()` which still compiles and is correct).

In `crates/web/src/adapter.rs`: remove the match arm:
```rust
            EmptyReason::NoBaseline => "no_baseline".to_string(),
```

- [ ] **Step 7: Remove `snapshot.no_baseline` assignment in scan.rs**

In `crates/cli/src/commands/scan.rs`, delete line ~479:
```rust
    snapshot.no_baseline = args.no_baseline;
```

Note: keep the `--no-baseline` CLI flag itself for now (removed in Task 3). This field assignment is the only thing that breaks.

- [ ] **Step 8: Clean up ALL test files**

Run `cargo check --workspace 2>&1 | grep 'no_baseline'` to find every remaining reference.

Remove `no_baseline: true` and `no_baseline: false` from struct literals in:
- `crates/pipeline/tests/service_intent_test.rs` (~11 occurrences)
- `crates/core/tests/fleet_merge_test.rs` (~5 occurrences)
- `crates/core/tests/fleet_orchestrator_test.rs` (~3 occurrences)
- `crates/core/tests/fleet_validate_test.rs` (~1 occurrence)
- `crates/refine/tests/cross_crate_integration_test.rs` (~5 occurrences)
- `crates/refine/tests/session_test.rs` (~2 occurrences)
- `crates/refine/tests/attention_test.rs` (~2 occurrences)
- `crates/cli/tests/refine_e2e_test.rs` (~1 occurrence)
- `crates/cli/src/commands/scan.rs` tests (~1 occurrence)
- `crates/collect/src/inspectors/rpm/mod.rs` tests (remove assertions on `rpm.no_baseline`)

Delete dedicated no-baseline test functions:
- `baseline_fmt.rs`: `section_lines_degraded_no_baseline`, `section_lines_skipped_no_baseline`
- `readme.rs`: tests that set `snap.no_baseline = true`
- `report.rs`: `test_report_no_baseline_panel_when_absent`
- `containerfile.rs`: `test_from_target_image_with_no_baseline_degraded`
- `reference.rs`: `test_no_baseline_returns_no_baseline`

- [ ] **Step 9: Verify clean build**

Run: `cargo test --workspace`
Expected: all tests pass. Zero references to `no_baseline` in production or test Rust code (except comments).

Run: `cargo clippy --workspace -- -D warnings`
Expected: zero warnings.

- [ ] **Step 10: Commit**

```bash
git add crates/
git commit -m "refactor: remove no_baseline from all Rust code

Removes no_baseline bool from InspectionSnapshot and RpmSection,
the RPM inspector derivation/warning, all renderer branches,
fleet merge logic, EmptyReason::NoBaseline, web adapter mapping,
and ~11 dedicated test functions plus ~25 test struct literals.

Part of mandatory baseline requirement."
```

---

### Task 3: Remove `--no-baseline` CLI flag and wire hard error with exit code 3 (cli crate)

**Owner:** Rust
**Files:**
- Modify: `crates/cli/src/commands/scan.rs`

- [ ] **Step 1: Remove the CLI flag**

Delete from `ScanArgs`:
```rust
    /// Skip baseline extraction (degraded classification mode)
    #[arg(long)]
    pub no_baseline: bool,
```

- [ ] **Step 2: Remove flag validation**

Delete the `base_image && no_baseline` conflict check (lines ~235-239).

- [ ] **Step 3: Extract resolution failure formatter and wire it**

Add a testable formatter to `crates/cli/src/commands/pull_failure.rs`:

```rust
/// Format a resolution failure (can't determine target image) into
/// a user-facing error with --base-image guidance.
pub fn format_resolution_error(cause: &str) -> String {
    let mut out = String::new();
    out.push_str("Error: could not determine target base image\n\n");
    out.push_str(&format!("  Cause:  {cause}\n\n"));
    out.push_str("  Specify the target image explicitly:\n");
    out.push_str("    inspectah scan --base-image <registry>/<image>:<tag>\n\n");
    out.push_str("  Example:\n");
    out.push_str("    inspectah scan --base-image quay.io/centos-bootc/centos-bootc:stream10\n");
    out.push_str("    inspectah scan --base-image registry.redhat.io/rhel10/rhel-bootc:10.2\n");
    out
}
```

Add unit test in `pull_failure.rs` `#[cfg(test)]` block:

```rust
#[test]
fn format_resolution_error_includes_base_image_guidance() {
    let msg = format_resolution_error("could not detect OS from /etc/os-release");
    assert!(msg.starts_with("Error: could not determine target base image"));
    assert!(msg.contains("--base-image"));
    assert!(msg.contains("centos-bootc"));
    assert!(msg.contains("rhel10"));
    assert!(msg.contains("could not detect OS"));
}
```

In `crates/cli/src/commands/scan.rs`, delete the `Err(e) if args.no_baseline` arm (line ~283). Replace the generic `Err(e) => return Err(e.into())` with:

```rust
        Err(e) => {
            let msg = pull_failure::format_resolution_error(&e.to_string());
            eprint!("{msg}");
            std::process::exit(1);
        }
```

This is the spec-required resolution-failure UX: exit code 1 with actionable `--base-image` guidance. This is a distinct error path from pull failure (exit 3) — the image ref couldn't even be determined, so there's nothing to pull. The formatter is unit-tested in `pull_failure.rs`; the wiring is verified manually in Task 8.

- [ ] **Step 4: Simplify baseline extraction match**

Change from:
```rust
    let baseline_data = match (&normalized_ref, args.no_baseline) {
        (Some(norm), false) => { /* pull logic */ Some(data) }
        (Some(_norm), true) => { eprintln!("...skipped..."); None }
        _ => None,
    };
```
to:
```rust
    let baseline_data = match &normalized_ref {
        Some(norm) => { /* pull logic */ Some(data) }
        None => None,
    };
```

- [ ] **Step 5: Wire pull failure classifier into error handling**

In the pull logic, replace the `.context("baseline extraction failed")?` with explicit classification:

```rust
// In each extract_baseline call site (TTY and non-TTY paths), replace:
//   .context("baseline extraction failed")?
// with:
match result_of_extract {
    Ok(data) => data,
    Err(_e) => {
        // Clear viewport if TTY and lines were rendered
        if ring_pos > 0 {
            pull_progress::viewport_cleanup(viewport_lines);
        }
        let stderr_combined = collected_lines.join("\n");
        let kind = pull_failure::classify_pull_failure(&stderr_combined);
        let msg = pull_failure::format_pull_error(
            kind,
            norm.as_str(),
            &stderr_combined,
        );
        eprint!("{msg}");
        std::process::exit(3);
    }
}
```

Apply the same pattern in both the TTY and non-TTY code paths. The key difference: instead of `?` propagating a generic error, catch it, classify, format, and exit 3.

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-cli`
Expected: pass.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy -p inspectah-cli -- -D warnings`
Expected: zero warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/cli/src/commands/scan.rs
git commit -m "feat(cli): remove --no-baseline, hard error on pull failure (exit 3)

Baseline extraction is now mandatory. Pull failures exit with
code 3 and a classified error message with per-category
remediation. The --no-baseline flag is removed.

Part of mandatory baseline requirement."
```

---

### Task 4: Bump schema version to 19 (core crate)

**Owner:** Rust
**Files:**
- Modify: `crates/core/src/snapshot.rs:21`

- [ ] **Step 1: Bump `SCHEMA_VERSION`**

```rust
// Before:
pub const SCHEMA_VERSION: u32 = 18;

// After:
pub const SCHEMA_VERSION: u32 = 19;
```

- [ ] **Step 2: Fix any hardcoded version assertions**

Run: `cargo test --workspace 2>&1 | grep 'FAIL\|schema'`

Update any tests that hardcode `18` to use `SCHEMA_VERSION` or `19`.

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/snapshot.rs
git commit -m "feat(core): bump SCHEMA_VERSION to 19

Breaking schema change. Old tarballs (schema ≤18, including
no-baseline artifacts) are rejected by the existing version
gate in validate.rs and snapshot.rs.

Part of mandatory baseline requirement."
```

---

### Task 5: Baseline requirement test suite (process + function boundary)

**Owner:** Rust
**Files:**
- Create: `crates/cli/tests/baseline_requirement_test.rs`
- Modify: `crates/cli/Cargo.toml` (add `assert_cmd` and `predicates` dev-dependencies)

**Test pyramid for this feature:**

| Layer | What's tested | Where | Automated? |
|-------|--------------|-------|------------|
| Process boundary | `--no-baseline` rejected (clap exit 2) | `tests/baseline_requirement_test.rs` | Yes — `assert_cmd` |
| Unit (function) | Pull failure classify → format output contract | `pull_failure.rs` `#[cfg(test)]` (Task 1) | Yes |
| Unit (function) | Resolution failure → `--base-image` guidance | `pull_failure.rs` `#[cfg(test)]` (Task 1) | Yes |
| Unit (function) | Credential redaction in final error excerpt | `pull_failure.rs` `#[cfg(test)]` (Task 1) | Yes |
| Unit (function) | Credential redaction in live callbacks | `pull_progress.rs` `#[cfg(test)]` (Task 1) | Yes — `Write` seam |
| Manual (root) | Pull failure → exit code 3 at process boundary | Task 8 Step 8 | No |
| Manual (root) | Resolution failure → exit code 1 at process boundary | Task 8 Step 8 | No |

No `lib.rs` split is needed. All function-boundary tests are unit tests inside their own modules. The integration test file only uses `assert_cmd` (no internal imports). Root-required process-boundary proofs are manual.

- [ ] **Step 1: Add dev-dependencies**

In `crates/cli/Cargo.toml`, add to `[dev-dependencies]`:
```toml
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 2: Create process-boundary test file**

`inspectah-cli` is a bin-only crate — integration tests in `crates/cli/tests/` cannot import internal modules. This test file uses ONLY `assert_cmd` (process-boundary) and does not import any `inspectah_cli` internals. All function-boundary tests for `pull_failure` and `pull_progress` live as `#[cfg(test)]` unit tests inside their respective modules (Task 1).

Create `crates/cli/tests/baseline_requirement_test.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

/// The `--no-baseline` flag must be rejected by clap after removal.
/// Runs without root — clap validates args before the root check.
#[test]
fn no_baseline_flag_rejected() {
    Command::cargo_bin("inspectah")
        .unwrap()
        .args(["scan", "--no-baseline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"))
        .code(2);
}
```

Note: process-boundary tests for exit code 3 (pull failure) and exit code 1 (resolution failure) require root and a deliberate failure scenario. These are verified manually in Task 8 Step 8. The function-boundary proof that the correct output is *produced* lives in the unit tests in `pull_failure.rs` (Task 1) and `scan.rs` (see Step 3 below).

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-cli --test baseline_requirement_test`
Expected: `no_baseline_flag_rejected` passes.

Run: `cargo test -p inspectah-cli -- pull_failure`
Expected: all unit tests pass (classifier, formatter, sanitizer, resolution error — these live in `pull_failure.rs` from Task 1).

Run: `cargo test -p inspectah-cli -- pull_progress`
Expected: live callback sanitization tests pass (from Task 1).

- [ ] **Step 4: Run full workspace tests**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/tests/baseline_requirement_test.rs crates/cli/Cargo.toml
git commit -m "test(cli): add process-boundary test for --no-baseline rejection

Uses assert_cmd to prove clap rejects --no-baseline at the
process boundary (exit 2). Function-boundary tests for pull
failure format, resolution failure guidance, credential
redaction, and message ordering live as unit tests in
pull_failure.rs and pull_progress.rs (Task 1).

Exit code 3 and exit code 1 process-boundary proofs require
root — documented for manual verification in Task 8.

Part of mandatory baseline requirement."
```

---

### Task 6: Frontend cleanup (web UI)

**Owner:** Frontend
**Files:**
- Modify: `crates/web/ui/src/components/MainContent.tsx`
- Modify: `crates/web/ui/src/components/__tests__/EmptyStates.test.tsx`

**Preservation note:** Do NOT touch `sysctl_no_baseline` in `attentionUtils.ts`, `SysctlSection.test.tsx`, or `api/types.ts` — that's a per-inspector triage concept, not the global flag.

- [ ] **Step 1: Remove no_baseline empty state in `MainContent.tsx`**

Remove or convert the "Baseline unavailable" banner (line ~324):
```tsx
title="Baseline unavailable — all added packages shown as NeedsReview"
```

Remove the `no_baseline:` empty-reason mapping (line ~507).

Decide: delete entirely (baseline is always present, this state is impossible), or convert to a defensive assertion/console.warn for an impossible state. Recommendation: delete entirely — dead code for an impossible state adds confusion.

- [ ] **Step 2: Remove no_baseline test in `EmptyStates.test.tsx`**

Delete the test (lines ~356-363):
```tsx
  it("renders no_baseline empty state", async () => {
    ...
        empty_reason: "no_baseline",
    ...
  });
```

- [ ] **Step 3: Run frontend tests**

Run from `crates/web/ui/`:
```bash
npx vitest run
```
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/
git commit -m "refactor(web): remove no_baseline empty state from frontend

Removes the 'Baseline unavailable' banner and no_baseline
empty-reason mapping from MainContent, plus the corresponding
test. sysctl_no_baseline (per-inspector triage) is preserved.

Part of mandatory baseline requirement."
```

---

### Task 7: Documentation, completions, and changelog

**Owner:** Docs
**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `docs/getting-started.md`
- Modify: `docs/how-to/baseline-subtraction.md`
- Modify: `docs/how-to/ci-integration.md`
- Modify: `docs/reference/cli.md`
- Modify: `docs/reference/configuration.md`
- Modify: `docs/reference/snapshot-schema.md`
- Modify: `docs/reference/triage-classification.md`
- Modify: `docs/explanation/triage-philosophy.md`
- Modify: `docs/diagrams/triage-decision-tree.html`
- Modify: `completions/inspectah.bash`
- Modify: `completions/inspectah.zsh`
- Modify: `completions/inspectah.fish`

- [ ] **Step 1: Update `README.md`**

Add prerequisites section and exit code table:

```markdown
### Prerequisites

- **Root access** — `inspectah scan` requires root privileges
- **Podman** — installed and available (`sudo dnf install podman`)
- **Target base image** — inspectah must be able to pull your target container image.
  For disconnected or air-gapped environments:
  - Pull the image on a connected machine: `podman save -o baseline.tar <image-ref>`
  - Transfer the tarball to the target host
  - Load it: `podman load -i baseline.tar`
  - Alternatively, use a local or mirror registry

### Exit Codes

| Code | Meaning |
|------|---------|
| 0    | Success — scan completed, report is trustworthy |
| 1    | General error (invalid arguments, missing permissions, etc.) |
| 2    | Incomplete scan — one or more inspectors failed, report has blind spots |
| 3    | Baseline pull failure — could not pull the target container image |
| 130  | Interrupted — scan was cancelled by the user (SIGINT / Ctrl-C) |
```

- [ ] **Step 2: Update `CHANGELOG.md`**

Add under `[Unreleased]`:

```markdown
### Removed
- **`--no-baseline` flag** — baseline extraction is now mandatory. Scans that cannot pull the target image exit with a clear error and remediation guidance. Use `--base-image` to override auto-resolution, or use `podman save`/`podman load` for disconnected environments.

### Changed
- **Schema version** bumped to 19. Tarballs from older schema versions are no longer loadable.
- **Exit codes** — pull failures now exit with code 3 (previously the scan would continue with degraded output).

### Added
- **Pull failure classification** — five error categories (registry unreachable, auth required, image not found, TLS/cert error, unknown) with tailored remediation guidance including disconnected-environment workarounds.
```

- [ ] **Step 3: Update docs files**

For each file, remove `--no-baseline` references and update to reflect mandatory baseline:

- `docs/getting-started.md:28` — remove the `--no-baseline` mention, replace with prerequisite about target image access
- `docs/how-to/baseline-subtraction.md:68,78,101` — remove `--no-baseline` examples, update the air-gapped section to describe `podman save`/`podman load` instead
- `docs/how-to/ci-integration.md:109` — remove `--no-baseline` from CI example, replace with a note about pre-staging the image
- `docs/reference/cli.md:52` — remove `--no-baseline` from the flags table
- `docs/reference/configuration.md:43` — remove `--no-baseline` from the options table
- `docs/reference/snapshot-schema.md:92` — remove `no_baseline` from the schema fields table, update schema version to 19
- `docs/reference/triage-classification.md:91` — update `PackageProvenanceUnavailable` text (this describes what happens when provenance is unknown, which can still happen for non-baseline reasons). Line 95 (`SysctlNoBaseline`) stays — it's the per-inspector concept.
- `docs/explanation/triage-philosophy.md:85` — update the prose about baseline availability
- `docs/diagrams/triage-decision-tree.html:113` — update the example text

- [ ] **Step 4: Regenerate shell completions**

Run from the repo root:
```bash
cargo run -p inspectah-cli -- completions bash > completions/inspectah.bash
cargo run -p inspectah-cli -- completions zsh > completions/inspectah.zsh
cargo run -p inspectah-cli -- completions fish > completions/inspectah.fish
```

Verify `--no-baseline` is absent:
```bash
grep -r 'no.baseline' completions/
```
Expected: zero results.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md docs/ completions/
git commit -m "docs: document mandatory baseline requirement

Updates README with prerequisites and exit code table. Removes
--no-baseline from all docs, CLI reference, completions, and
schema reference. Updates CHANGELOG with removal, schema bump,
and pull failure classification.

Part of mandatory baseline requirement."
```

---

### Task 8: Final verification

**Owner:** Rust

- [ ] **Step 1: Verify zero Rust references to `no_baseline`**

```bash
grep -rn 'no_baseline' crates/ --include='*.rs'
```
Expected: zero results.

- [ ] **Step 2: Verify `sysctl_no_baseline` preserved in frontend**

```bash
grep -rn 'sysctl_no_baseline' crates/web/ui/
```
Expected: hits in `attentionUtils.ts`, `SysctlSection.test.tsx`, `api/types.ts`. These are correct — do NOT remove.

- [ ] **Step 3: Verify zero doc/completion references to `--no-baseline`**

```bash
grep -rn 'no.baseline\|--no-baseline' docs/ completions/ README.md
```
Expected: zero results (except `SysctlNoBaseline` in `triage-classification.md` which is the per-inspector concept).

- [ ] **Step 4: Full test suite**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Full clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: zero warnings.

- [ ] **Step 6: Verify schema version**

```bash
grep 'SCHEMA_VERSION' crates/core/src/snapshot.rs
```
Expected: `pub const SCHEMA_VERSION: u32 = 19;`

- [ ] **Step 7: Verify `--no-baseline` rejected by clap**

```bash
cargo build -p inspectah-cli
./target/debug/inspectah scan --no-baseline 2>&1
```
Expected: clap error about unknown argument. Exit code 2 (clap default).

- [ ] **Step 8: Manual process-boundary test for exit code 3 (requires root)**

On a test VM or system with root access, verify the pull failure exit code:
```bash
sudo ./target/debug/inspectah scan --base-image nonexistent.invalid/no-such-image:v999
echo "Exit code: $?"
```
Expected: exit code 3, classified error message ("cannot reach registry" or "image not found"), disconnected hint present.

This test cannot be automated in CI (requires root + deliberate pull failure). The `assert_cmd` test in Task 5 proves the clap-level boundary; this step proves the runtime boundary.

- [ ] **Step 9: Frontend tests**

```bash
cd crates/web/ui && npx vitest run
```
Expected: all pass.
