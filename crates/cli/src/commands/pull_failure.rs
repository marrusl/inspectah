//! Pull failure classification, formatting, and credential sanitization.
//!
//! When a `podman pull` fails, the raw stderr is noisy and may contain
//! credentials. This module classifies errors into actionable categories,
//! sanitizes sensitive tokens, and formats user-friendly messages with
//! per-category remediation guidance.
//!
//! Nothing calls the classifier/formatter yet — Task 3 wires them into
//! the scan flow. The sanitizer is used by `pull_progress` callbacks.

use std::fmt;

/// Classified pull failure categories, ordered by diagnostic priority.
// Used by Task 3 (scan flow integration); not yet wired.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullFailureKind {
    /// TLS certificate verification failed (self-signed, expired, etc.)
    TlsCertError,
    /// Registry requires authentication but credentials are missing/invalid.
    AuthRequired,
    /// Registry host could not be reached (DNS, network, firewall).
    RegistryUnreachable,
    /// Image reference does not exist in the registry.
    ImageNotFound,
    /// Unrecognized failure — show raw stderr.
    Unknown,
}

impl fmt::Display for PullFailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TlsCertError => write!(f, "TLS certificate error"),
            Self::AuthRequired => write!(f, "authentication required"),
            Self::RegistryUnreachable => write!(f, "registry unreachable"),
            Self::ImageNotFound => write!(f, "image not found"),
            Self::Unknown => write!(f, "unknown pull error"),
        }
    }
}

/// Classify a podman pull failure from its stderr output.
///
/// Priority order: TLS > Auth > Registry > NotFound > Unknown.
/// This order matters — a TLS error may also mention auth, but the root
/// cause is the certificate, not the credentials.
// Used by Task 3 (scan flow integration); not yet wired.
#[allow(dead_code)]
pub fn classify_pull_failure(stderr: &str) -> PullFailureKind {
    let lower = stderr.to_lowercase();

    // TLS first — most specific, often masks auth errors
    if lower.contains("certificate")
        || lower.contains("x509")
        || lower.contains("tls")
        || lower.contains("ssl")
    {
        return PullFailureKind::TlsCertError;
    }

    // Auth — 401/403 or explicit auth messages
    if lower.contains("unauthorized")
        || lower.contains("authentication required")
        || lower.contains("403")
        || lower.contains("denied")
        || lower.contains("login")
    {
        return PullFailureKind::AuthRequired;
    }

    // Registry unreachable — DNS/network failures
    if lower.contains("no such host")
        || lower.contains("connection refused")
        || lower.contains("timeout")
        || lower.contains("could not resolve")
        || lower.contains("network is unreachable")
        || lower.contains("i/o timeout")
    {
        return PullFailureKind::RegistryUnreachable;
    }

    // Image not found — 404 or manifest missing
    if lower.contains("manifest unknown")
        || lower.contains("not found")
        || lower.contains("404")
        || lower.contains("name unknown")
    {
        return PullFailureKind::ImageNotFound;
    }

    PullFailureKind::Unknown
}

/// Sanitize stderr by redacting bearer tokens, basic auth, and authorization headers.
///
/// This prevents credential leakage in error output shown to users or
/// collected in diagnostic bundles.
pub fn sanitize_stderr(s: &str) -> String {
    let mut result = s.to_string();

    // Bearer tokens: Bearer <token>
    // Case-insensitive match for the keyword, greedy token capture.
    // We handle the first occurrence — multiple bearer tokens in one
    // stderr blob is not a realistic scenario.
    if let Some(pos) = result.to_lowercase().find("bearer ") {
        let token_start = pos + 7; // len("bearer ")
        if token_start < result.len() {
            let token_end = result[token_start..]
                .find(|c: char| c.is_whitespace())
                .map(|p| token_start + p)
                .unwrap_or(result.len());
            if token_end > token_start {
                result.replace_range(token_start..token_end, "[REDACTED]");
            }
        }
    }

    // Basic auth in URLs: https://user:pass@host -> https://[REDACTED]@host
    if let Some(start) = result.find("://").map(|p| p + 3)
        && start < result.len()
        && let Some(at_pos) = result[start..].find('@')
    {
        let abs_at = start + at_pos;
        // Only redact if there's no slash before the @
        let before_at = &result[start..abs_at];
        if !before_at.contains('/') && !before_at.is_empty() {
            result.replace_range(start..abs_at, "[REDACTED]");
        }
    }

    // Authorization headers: Authorization: <value>
    let auth_lower = result.to_lowercase();
    if let Some(pos) = auth_lower.find("authorization:") {
        let value_start = pos + 14; // len("authorization:")
        // Skip whitespace after colon
        let trimmed_start = result[value_start..]
            .find(|c: char| !c.is_whitespace())
            .map(|p| value_start + p)
            .unwrap_or(value_start);
        if trimmed_start < result.len() {
            // Value extends to end of line or end of string
            let value_end = result[trimmed_start..]
                .find('\n')
                .map(|p| trimmed_start + p)
                .unwrap_or(result.len());
            if value_end > trimmed_start {
                result.replace_range(trimmed_start..value_end, "[REDACTED]");
            }
        }
    }

    result
}

/// Extract the registry hostname (with optional port) from an image reference.
///
/// Returns `None` for bare names without a registry prefix (e.g., `fedora:latest`).
#[allow(dead_code)]
fn registry_from_ref(image_ref: &str) -> Option<&str> {
    // Strip digest (@sha256:...) first
    let name = image_ref.split('@').next().unwrap_or(image_ref);

    // Strip tag — the tag colon is always after the last '/'
    let name = if let Some(slash_pos) = name.rfind('/') {
        // Only strip colon after the last slash (that's the tag separator)
        if let Some(colon_pos) = name[slash_pos..].find(':') {
            &name[..slash_pos + colon_pos]
        } else {
            name
        }
    } else {
        // No slash — colon is the tag separator (e.g., "fedora:latest")
        name.split(':').next().unwrap_or(name)
    };

    // A registry prefix contains a dot, colon (port), or is "localhost"
    if let Some(first_segment) = name.split('/').next()
        && (first_segment.contains('.')
            || first_segment.contains(':')
            || first_segment == "localhost")
    {
        return Some(first_segment);
    }
    None
}

/// Truncate stderr to a reasonable length for display.
///
/// Keeps the first `max_lines` lines and appends a count of omitted lines.
#[allow(dead_code)]
fn truncate_stderr(stderr: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    if lines.len() <= max_lines {
        return stderr.to_string();
    }
    let kept: Vec<&str> = lines[..max_lines].to_vec();
    let omitted = lines.len() - max_lines;
    format!("{}\n  ... ({omitted} more lines)", kept.join("\n"))
}

/// Format a structured pull error message with per-category remediation.
///
/// The output includes:
/// - A headline identifying the failure kind and image
/// - Sanitized stderr excerpt
/// - Remediation steps specific to the error category
/// - A disconnected-system hint (inspectah may run on air-gapped hosts)
// Used by Task 3 (scan flow integration); not yet wired.
#[allow(dead_code)]
pub fn format_pull_error(kind: &PullFailureKind, image_ref: &str, raw_stderr: &str) -> String {
    let sanitized = sanitize_stderr(raw_stderr);
    let truncated = truncate_stderr(&sanitized, 10);
    let registry = registry_from_ref(image_ref).unwrap_or("the registry");

    let mut msg = format!(
        "Failed to pull baseline image: {kind}\n\
         Image: {image_ref}\n\n\
         {truncated}\n"
    );

    match kind {
        PullFailureKind::TlsCertError => {
            msg.push_str(&format!(
                "\nRemediation:\n\
                 - Verify the TLS certificate for {registry}\n\
                 - If using a private registry, ensure its CA is trusted on this host\n\
                 - For testing only: podman pull --tls-verify=false (not recommended)\n"
            ));
        }
        PullFailureKind::AuthRequired => {
            msg.push_str(&format!(
                "\nRemediation:\n\
                 - Run: podman login {registry}\n\
                 - Check that credentials are current and have pull access\n\
                 - For Red Hat registries: verify subscription or token at access.redhat.com\n"
            ));
        }
        PullFailureKind::RegistryUnreachable => {
            msg.push_str(&format!(
                "\nRemediation:\n\
                 - Check network connectivity to {registry}\n\
                 - Verify DNS resolution: host {registry}\n\
                 - Check firewall rules and proxy configuration\n"
            ));
        }
        PullFailureKind::ImageNotFound => {
            msg.push_str(&format!(
                "\nRemediation:\n\
                 - Verify the image reference: {image_ref}\n\
                 - Check that the tag or digest exists in the registry\n\
                 - Use: podman search {registry}/<name> to find available tags\n"
            ));
        }
        PullFailureKind::Unknown => {
            msg.push_str(
                "\nRemediation:\n\
                 - Review the error output above for details\n\
                 - Try: podman pull <image> manually for more diagnostics\n",
            );
        }
    }

    msg.push_str(
        "\nIf this system is disconnected or air-gapped, use --no-baseline to skip\n\
         the pull and run inspectah without baseline comparison.",
    );

    msg
}

/// Format an error message for when the target image cannot be resolved.
///
/// Guides the user to provide `--base-image` explicitly with example references.
// Used by Task 3 (scan flow integration); not yet wired.
#[allow(dead_code)]
pub fn format_resolution_error(cause: &str) -> String {
    format!(
        "Could not determine target image for baseline comparison.\n\
         Cause: {cause}\n\n\
         Provide the target image explicitly:\n\
         \n\
           inspectah scan --base-image quay.io/centos-bootc/centos-bootc:stream10\n\
           inspectah scan --base-image registry.redhat.io/rhel10/rhel-bootc:10.2\n\
         \n\
         Use --no-baseline to skip baseline comparison entirely."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Classifier tests ──────────────────────────────────────────

    #[test]
    fn classify_tls_x509() {
        let kind = classify_pull_failure("error: x509: certificate signed by unknown authority");
        assert_eq!(kind, PullFailureKind::TlsCertError);
    }

    #[test]
    fn classify_tls_certificate() {
        let kind = classify_pull_failure("Error: tls: failed to verify certificate: x509: ...");
        assert_eq!(kind, PullFailureKind::TlsCertError);
    }

    #[test]
    fn classify_auth_unauthorized() {
        let kind = classify_pull_failure(
            "Error: initializing source: unauthorized: authentication required",
        );
        assert_eq!(kind, PullFailureKind::AuthRequired);
    }

    #[test]
    fn classify_auth_denied() {
        let kind =
            classify_pull_failure("Error: denied: requested access to the resource is denied");
        assert_eq!(kind, PullFailureKind::AuthRequired);
    }

    #[test]
    fn classify_registry_no_host() {
        let kind = classify_pull_failure(
            "Error: initializing source: pinging container registry: no such host",
        );
        assert_eq!(kind, PullFailureKind::RegistryUnreachable);
    }

    #[test]
    fn classify_image_not_found() {
        let kind = classify_pull_failure("Error: manifest unknown: manifest unknown");
        assert_eq!(kind, PullFailureKind::ImageNotFound);
    }

    #[test]
    fn classify_unknown() {
        let kind = classify_pull_failure("Error: something completely unexpected happened");
        assert_eq!(kind, PullFailureKind::Unknown);
    }

    #[test]
    fn classify_priority_tls_over_auth() {
        // Contains both TLS and auth keywords — TLS should win
        let kind = classify_pull_failure(
            "Error: unauthorized: tls: certificate verify failed: x509: unknown authority",
        );
        assert_eq!(kind, PullFailureKind::TlsCertError);
    }

    // ── Sanitizer tests ──────────────────────────────────────────

    #[test]
    fn sanitize_bearer_token() {
        let input =
            "Authorization failed: Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.payload.sig rest";
        let result = sanitize_stderr(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(result.contains("rest"));
    }

    #[test]
    fn sanitize_basic_auth_in_url() {
        let input = "pulling from https://user:s3cretP4ss@registry.example.com/repo:latest";
        let result = sanitize_stderr(input);
        assert!(result.contains("[REDACTED]@registry.example.com"));
        assert!(!result.contains("user:s3cretP4ss"));
    }

    #[test]
    fn sanitize_authorization_header() {
        let input = "header sent: Authorization: Basic dXNlcjpwYXNz\nnext line";
        let result = sanitize_stderr(input);
        assert!(result.contains("Authorization:"));
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("dXNlcjpwYXNz"));
        assert!(result.contains("next line"));
    }

    #[test]
    fn sanitize_no_credentials() {
        let input = "Copying blob sha256:abc123 done\nWriting manifest";
        let result = sanitize_stderr(input);
        assert_eq!(result, input);
    }

    // ── Formatter tests ──────────────────────────────────────────

    #[test]
    fn format_tls_error_has_remediation() {
        let msg = format_pull_error(
            &PullFailureKind::TlsCertError,
            "registry.example.com/image:latest",
            "x509: certificate error",
        );
        assert!(msg.contains("TLS certificate error"));
        assert!(msg.contains("registry.example.com"));
        assert!(msg.contains("Remediation"));
        assert!(msg.contains("CA is trusted"));
    }

    #[test]
    fn format_auth_error_has_login_hint() {
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "registry.redhat.io/rhel10/rhel-bootc:10.2",
            "unauthorized: authentication required",
        );
        assert!(msg.contains("podman login"));
        assert!(msg.contains("registry.redhat.io"));
    }

    #[test]
    fn format_registry_error_has_connectivity_hint() {
        let msg = format_pull_error(
            &PullFailureKind::RegistryUnreachable,
            "quay.io/centos-bootc/centos-bootc:stream10",
            "no such host",
        );
        assert!(msg.contains("network connectivity"));
        assert!(msg.contains("quay.io"));
    }

    #[test]
    fn format_not_found_has_search_hint() {
        let msg = format_pull_error(
            &PullFailureKind::ImageNotFound,
            "quay.io/centos-bootc/centos-bootc:nonexistent",
            "manifest unknown",
        );
        assert!(msg.contains("podman search"));
        assert!(msg.contains("tag or digest"));
    }

    #[test]
    fn format_unknown_has_manual_hint() {
        let msg = format_pull_error(
            &PullFailureKind::Unknown,
            "example.com/image:v1",
            "something weird",
        );
        assert!(msg.contains("podman pull"));
        assert!(msg.contains("manually"));
    }

    #[test]
    fn format_always_has_disconnect_hint() {
        let msg = format_pull_error(&PullFailureKind::Unknown, "example.com/image:v1", "error");
        assert!(msg.contains("--no-baseline"));
        assert!(msg.contains("disconnected"));
    }

    #[test]
    fn format_sanitizes_credentials() {
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "registry.example.com/image:latest",
            "failed with Bearer eyJhbGciOiJSUzI1NiJ9.payload.sig",
        );
        assert!(!msg.contains("eyJhbGciOiJSUzI1NiJ9"));
        assert!(msg.contains("[REDACTED]"));
    }

    #[test]
    fn format_error_ordering() {
        // Verify the message structure: headline, stderr, remediation, disconnect
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "quay.io/test:latest",
            "unauthorized",
        );
        let headline_pos = msg.find("Failed to pull").expect("headline");
        let stderr_pos = msg.find("unauthorized").expect("stderr");
        let remediation_pos = msg.find("Remediation").expect("remediation");
        let disconnect_pos = msg.find("disconnected").expect("disconnect");
        assert!(headline_pos < stderr_pos);
        assert!(stderr_pos < remediation_pos);
        assert!(remediation_pos < disconnect_pos);
    }

    // ── Resolution error test ────────────────────────────────────

    #[test]
    fn resolution_error_has_examples() {
        let msg = format_resolution_error("os-release missing IMAGE_REF");
        assert!(msg.contains("--base-image"));
        assert!(msg.contains("quay.io/centos-bootc/centos-bootc:stream10"));
        assert!(msg.contains("registry.redhat.io/rhel10/rhel-bootc:10.2"));
        assert!(msg.contains("--no-baseline"));
        assert!(msg.contains("os-release missing IMAGE_REF"));
    }

    // ── Truncation tests ─────────────────────────────────────────

    #[test]
    fn truncate_short_stderr() {
        let input = "line 1\nline 2\nline 3";
        let result = truncate_stderr(input, 10);
        assert_eq!(result, input);
    }

    #[test]
    fn truncate_long_stderr() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
        let input = lines.join("\n");
        let result = truncate_stderr(&input, 5);
        assert!(result.contains("line 0"));
        assert!(result.contains("line 4"));
        assert!(!result.contains("line 5"));
        assert!(result.contains("15 more lines"));
    }

    // ── Helper tests ─────────────────────────────────────────────

    #[test]
    fn registry_from_ref_qualified() {
        assert_eq!(
            registry_from_ref("registry.redhat.io/rhel10/rhel-bootc:10.2"),
            Some("registry.redhat.io")
        );
    }

    #[test]
    fn registry_from_ref_quay() {
        assert_eq!(
            registry_from_ref("quay.io/centos-bootc/centos-bootc:stream10"),
            Some("quay.io")
        );
    }

    #[test]
    fn registry_from_ref_bare() {
        assert_eq!(registry_from_ref("fedora:latest"), None);
    }

    #[test]
    fn registry_from_ref_localhost() {
        assert_eq!(
            registry_from_ref("localhost/myimage:test"),
            Some("localhost")
        );
    }

    #[test]
    fn registry_from_ref_with_port() {
        assert_eq!(
            registry_from_ref("myhost:5000/image:tag"),
            Some("myhost:5000")
        );
    }
}
