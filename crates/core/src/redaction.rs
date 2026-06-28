/// Scrubs secret-like values from compose YAML content.
///
/// Replaces values of environment variables whose names match
/// secret patterns with `<REDACTED>`. Handles both `KEY=VALUE` and
/// `KEY: value` patterns within `environment:` blocks.
///
/// Lives in inspectah-core so both inspectah-collect and
/// inspectah-refine can use it without cross-crate dependency issues.
pub fn scrub_compose_secrets(content: &str) -> String {
    const SECRET_PATTERNS: &[&str] = &[
        "PASSWORD",
        "PASSWD",
        "SECRET",
        "TOKEN",
        "API_KEY",
        "PRIVATE_KEY",
        "AUTH",
        "CREDENTIAL",
    ];

    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            result.push_str(line);
            result.push('\n');
            continue;
        }
        let upper = trimmed.to_uppercase();
        let is_secret = SECRET_PATTERNS
            .iter()
            .any(|pat| upper.contains(pat) && (trimmed.contains('=') || trimmed.contains(':')));
        if is_secret {
            // Replace the value portion while preserving indentation and key.
            if let Some(eq_pos) = trimmed.find('=') {
                let indent = &line[..line.len() - line.trim_start().len()];
                let key = &trimmed[..eq_pos + 1];
                result.push_str(indent);
                result.push_str(key);
                result.push_str("<REDACTED>");
                result.push('\n');
            } else if let Some(colon_pos) = trimmed.find(':') {
                let indent = &line[..line.len() - line.trim_start().len()];
                let key = &trimmed[..colon_pos + 1];
                result.push_str(indent);
                result.push_str(key);
                result.push_str(" <REDACTED>");
                result.push('\n');
            } else {
                result.push_str(line);
                result.push('\n');
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    // Remove trailing newline added by the loop if original didn't have one.
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_compose_secrets_redacts_eq_style() {
        let input =
            "services:\n  web:\n    environment:\n      DB_PASSWORD=hunter2\n      APP_PORT=8080\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(scrubbed.contains("DB_PASSWORD=<REDACTED>"));
        assert!(scrubbed.contains("APP_PORT=8080")); // not secret
        assert!(!scrubbed.contains("hunter2"));
    }

    #[test]
    fn scrub_compose_secrets_redacts_colon_style() {
        let input =
            "services:\n  db:\n    environment:\n      SECRET_KEY: my-secret\n      LANG: en_US\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(scrubbed.contains("SECRET_KEY: <REDACTED>"));
        assert!(scrubbed.contains("LANG: en_US")); // not secret
        assert!(!scrubbed.contains("my-secret"));
    }

    #[test]
    fn scrub_compose_secrets_preserves_comments_and_blanks() {
        let input = "# A comment\n\nservices:\n  app:\n    image: nginx\n";
        let scrubbed = scrub_compose_secrets(input);
        assert_eq!(input, scrubbed);
    }

    #[test]
    fn scrub_compose_secrets_handles_api_token() {
        let input = "    API_TOKEN=abc123\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(scrubbed.contains("API_TOKEN=<REDACTED>"));
    }
}
