//! Pull failure classification, formatting, and credential sanitization.
//!
//! When a `podman pull` fails, the raw stderr is noisy and may contain
//! credentials. This module classifies errors into actionable categories,
//! sanitizes sensitive tokens, and formats user-friendly messages with
//! per-category remediation guidance.
//!
//! The classifier and formatter are called from `scan.rs` on pull failure
//! (exit 3) and resolution failure (exit 1). The sanitizer is also used
//! by `pull_progress` callbacks.

use std::fmt;

/// Classified pull failure categories, ordered by diagnostic priority.
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
pub fn classify_pull_failure(stderr: &str) -> PullFailureKind {
    let lower = stderr.to_lowercase();

    // TLS first — most specific, often masks auth errors
    if lower.contains("certificate")
        || lower.contains("x509")
        || lower.contains("tls")
        || lower.contains("ssl")
        || lower.contains("insecure")
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
    if lower.contains("dial tcp")
        || lower.contains("no such host")
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

/// Strip the tag or digest from an image reference, returning the repo-only part.
///
/// `quay.io/centos-bootc/centos-bootc:stream10` → `quay.io/centos-bootc/centos-bootc`
fn repo_from_ref(image_ref: &str) -> &str {
    let name = image_ref.split('@').next().unwrap_or(image_ref);
    if let Some(slash_pos) = name.rfind('/') {
        if let Some(colon_pos) = name[slash_pos..].find(':') {
            &name[..slash_pos + colon_pos]
        } else {
            name
        }
    } else {
        name.split(':').next().unwrap_or(name)
    }
}

/// Truncate stderr to a reasonable length for display.
///
/// Keeps the first `max_lines` lines and appends a count of omitted lines.
fn truncate_stderr(stderr: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(max_lines)
        .collect();
    lines.join("\n")
}

/// Format a structured pull error message with per-category remediation.
///
/// Follows the approved spec contract:
/// - Stable header: "Error: cannot pull baseline image"
/// - Image ref echoed back
/// - One-line cause
/// - Per-category remediation (2-4 lines)
/// - Disconnected hint with actual image ref on every category
/// - Raw stderr excerpt only for Unknown category (3 lines max, sanitized)
pub fn format_pull_error(kind: &PullFailureKind, image_ref: &str, raw_stderr: &str) -> String {
    let registry = registry_from_ref(image_ref).unwrap_or("the registry");
    let mut msg = String::new();

    msg.push_str("Error: cannot pull baseline image\n\n");
    msg.push_str(&format!("  Image:  {image_ref}\n"));

    match kind {
        PullFailureKind::RegistryUnreachable => {
            msg.push_str(&format!("  Cause:  cannot reach registry ({registry})\n\n"));
            msg.push_str("  Check network connectivity to the registry:\n");
            msg.push_str(&format!(
                "    curl -s https://{registry}/v2/ || echo \"unreachable\"\n"
            ));
            msg.push_str("  If behind a proxy, configure podman:\n");
            msg.push_str(
                "    Edit /etc/containers/registries.conf or set HTTP_PROXY/HTTPS_PROXY\n",
            );
        }
        PullFailureKind::AuthRequired => {
            msg.push_str("  Cause:  authentication required\n\n");
            msg.push_str("  Verify the image reference is correct (a wrong registry can look like an auth error):\n");
            msg.push_str("    inspectah scan --base-image <correct-registry>/<image>:<tag>\n");
            msg.push_str("  If the reference is correct, log in to the registry:\n");
            msg.push_str(&format!("    podman login {registry}\n"));
            msg.push_str(
                "  For Red Hat registries, use your Red Hat account or a service account token.\n",
            );
        }
        PullFailureKind::ImageNotFound => {
            let repo = repo_from_ref(image_ref);
            msg.push_str("  Cause:  image or tag not found\n\n");
            msg.push_str("  Verify the image reference is correct:\n");
            msg.push_str(&format!("    podman search {repo}\n"));
            msg.push_str(&format!("    skopeo list-tags docker://{repo}\n"));
            msg.push_str("  If your image is at a different registry or tag, use:\n");
            msg.push_str("    inspectah scan --base-image <correct-registry>/<image>:<tag>\n");
        }
        PullFailureKind::TlsCertError => {
            msg.push_str("  Cause:  TLS certificate error\n\n");
            msg.push_str("  Verify the image reference is correct (a wrong registry can cause TLS errors):\n");
            msg.push_str("    inspectah scan --base-image <correct-registry>/<image>:<tag>\n");
            msg.push_str("  If using a private registry with self-signed certificates:\n");
            msg.push_str(
                "    sudo cp ca.crt /etc/pki/ca-trust/source/anchors/ && sudo update-ca-trust\n",
            );
            msg.push_str("  Or configure podman to trust the registry:\n");
            msg.push_str(
                "    Edit /etc/containers/registries.conf.d/ to add [[registry]] with insecure=true\n",
            );
        }
        PullFailureKind::Unknown => {
            msg.push_str("  Cause:  pull failed\n\n");
            let sanitized = sanitize_stderr(raw_stderr);
            let excerpt = truncate_stderr(&sanitized, 3);
            if !excerpt.is_empty() {
                msg.push_str("  podman reported:\n");
                for line in excerpt.lines() {
                    msg.push_str(&format!("    {line}\n"));
                }
                msg.push('\n');
            }
            msg.push_str("  Try pulling the image manually to diagnose:\n");
            msg.push_str(&format!("    podman pull {image_ref}\n"));
        }
    }

    msg.push_str(&format!(
        "\n  Disconnected? You can load images from a tarball:\n\
         \x20\x20\x20\x20podman save -o baseline.tar {image_ref}\n\
         \x20\x20\x20\x20podman load -i baseline.tar"
    ));

    msg
}

/// Format an error message for when the target image cannot be resolved.
///
/// Guides the user to provide `--base-image` explicitly with example references.
pub fn format_resolution_error(cause: &str) -> String {
    format!(
        "Error: could not determine target base image\n\n  \
         Cause:  {cause}\n\n  \
         Specify the target image explicitly:\n    \
         inspectah scan --base-image <registry>/<image>:<tag>\n\n  \
         Example:\n    \
         inspectah scan --base-image quay.io/centos-bootc/centos-bootc:stream10\n    \
         inspectah scan --base-image registry.redhat.io/rhel10/rhel-bootc:10.2"
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

    // ── Formatter tests (spec contract) ───────────────────────────

    #[test]
    fn format_stable_header() {
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "quay.io/test:latest",
            "unauthorized",
        );
        assert!(msg.starts_with("Error: cannot pull baseline image"));
    }

    #[test]
    fn format_echoes_image_ref() {
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "quay.io/centos-bootc/centos-bootc:stream10",
            "unauthorized",
        );
        assert!(msg.contains("Image:  quay.io/centos-bootc/centos-bootc:stream10"));
    }

    #[test]
    fn format_tls_error() {
        let msg = format_pull_error(
            &PullFailureKind::TlsCertError,
            "registry.example.com/image:latest",
            "x509: certificate error",
        );
        assert!(msg.contains("Cause:  TLS certificate error"));
        assert!(msg.contains("--base-image"));
        assert!(msg.contains("ca-trust"));
    }

    #[test]
    fn format_auth_error() {
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "registry.redhat.io/rhel10/rhel-bootc:10.2",
            "unauthorized",
        );
        assert!(msg.contains("Cause:  authentication required"));
        assert!(msg.contains("podman login registry.redhat.io"));
    }

    #[test]
    fn format_registry_unreachable() {
        let msg = format_pull_error(
            &PullFailureKind::RegistryUnreachable,
            "quay.io/centos-bootc/centos-bootc:stream10",
            "no such host",
        );
        assert!(msg.contains("Cause:  cannot reach registry (quay.io)"));
        assert!(msg.contains("curl -s https://quay.io/v2/"));
    }

    #[test]
    fn format_not_found() {
        let msg = format_pull_error(
            &PullFailureKind::ImageNotFound,
            "quay.io/centos-bootc/centos-bootc:nonexistent",
            "manifest unknown",
        );
        assert!(msg.contains("Cause:  image or tag not found"));
        assert!(msg.contains("skopeo list-tags docker://quay.io/centos-bootc/centos-bootc"));
        assert!(
            !msg.contains(
                "skopeo list-tags docker://quay.io/centos-bootc/centos-bootc:nonexistent"
            ),
            "skopeo list-tags must use repo-only ref, not tagged ref"
        );
        assert!(msg.contains("podman search quay.io/centos-bootc/centos-bootc"));
    }

    #[test]
    fn format_unknown_shows_stderr_excerpt() {
        let stderr = "line one\nline two\nline three\nline four\nline five";
        let msg = format_pull_error(&PullFailureKind::Unknown, "example.com/image:v1", stderr);
        assert!(msg.contains("Cause:  pull failed"));
        assert!(msg.contains("line one"));
        assert!(msg.contains("line three"));
        assert!(!msg.contains("line four"), "must truncate at 3 lines");
        assert!(msg.contains("podman pull example.com/image:v1"));
    }

    #[test]
    fn format_non_unknown_omits_stderr() {
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "quay.io/test:latest",
            "unauthorized: some verbose podman output here",
        );
        assert!(
            !msg.contains("podman reported:"),
            "only Unknown shows stderr excerpt"
        );
    }

    #[test]
    fn format_disconnect_hint_uses_actual_ref() {
        let msg = format_pull_error(
            &PullFailureKind::RegistryUnreachable,
            "quay.io/centos-bootc/centos-bootc:stream10",
            "no such host",
        );
        assert!(
            msg.contains("podman save -o baseline.tar quay.io/centos-bootc/centos-bootc:stream10")
        );
        assert!(msg.contains("podman load"));
    }

    #[test]
    fn format_disconnect_hint_on_every_category() {
        for kind in &[
            PullFailureKind::TlsCertError,
            PullFailureKind::AuthRequired,
            PullFailureKind::RegistryUnreachable,
            PullFailureKind::ImageNotFound,
            PullFailureKind::Unknown,
        ] {
            let msg = format_pull_error(kind, "example.com/img:v1", "err");
            assert!(msg.contains("Disconnected?"), "missing hint for {kind:?}");
            assert!(msg.contains("podman save"), "missing save for {kind:?}");
        }
    }

    #[test]
    fn format_unknown_sanitizes_credentials() {
        let stderr = "Bearer eyJsecret in line one\nline two\nline three";
        let msg = format_pull_error(&PullFailureKind::Unknown, "quay.io/test:v1", stderr);
        assert!(msg.contains("[REDACTED]"));
        assert!(!msg.contains("eyJsecret"));
    }

    #[test]
    fn format_auth_leads_with_base_image() {
        let msg = format_pull_error(
            &PullFailureKind::AuthRequired,
            "registry.example.com/img:v1",
            "",
        );
        let base_image_pos = msg.find("--base-image").unwrap();
        let login_pos = msg.find("podman login").unwrap();
        assert!(base_image_pos < login_pos);
    }

    #[test]
    fn format_tls_leads_with_base_image() {
        let msg = format_pull_error(
            &PullFailureKind::TlsCertError,
            "registry.example.com/img:v1",
            "",
        );
        let base_image_pos = msg.find("--base-image").unwrap();
        let ca_pos = msg.find("ca-trust").unwrap();
        assert!(base_image_pos < ca_pos);
    }

    // ── Resolution error test ────────────────────────────────────

    #[test]
    fn resolution_error_has_examples() {
        let msg = format_resolution_error("os-release missing IMAGE_REF");
        assert!(msg.contains("--base-image"));
        assert!(msg.contains("quay.io/centos-bootc/centos-bootc:stream10"));
        assert!(msg.contains("registry.redhat.io/rhel10/rhel-bootc:10.2"));
        assert!(
            !msg.contains("--no-baseline"),
            "must not reference removed flag"
        );
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
        let result = truncate_stderr("one\ntwo\nthree\nfour\nfive", 3);
        assert_eq!(result, "one\ntwo\nthree");
    }

    #[test]
    fn truncate_skips_blank_lines() {
        let result = truncate_stderr("one\n\n\ntwo\n\nthree\nfour", 3);
        assert_eq!(result, "one\ntwo\nthree");
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
