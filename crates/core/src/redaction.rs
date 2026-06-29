/// Scrubs secret-like values from compose YAML content.
///
/// Targets two categories of secrets:
/// 1. **Named secrets in env-var assignments:** Lines with `=` or `: ` where
///    the key matches secret patterns (PASSWORD, TOKEN, etc.).
/// 2. **URL-embedded credentials:** Values containing `://user:pass@host`
///    DSN patterns (DATABASE_URL, REDIS_URL, etc.).
///
/// Preserves compose YAML structure: top-level `secrets:` blocks and
/// `file:` references are NOT redacted (they are compose structure, not
/// env var values).
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

    /// Check if a line looks like an env-var assignment (KEY=value or
    /// KEY: value with indentation suggesting it's inside an environment
    /// block, not a top-level YAML key like `secrets:` or `services:`).
    fn is_env_var_line(trimmed: &str) -> bool {
        // Lines with `=` are always env-var assignments in compose YAML.
        if trimmed.contains('=') {
            return true;
        }
        // Lines with `:` could be YAML structure or `KEY: value` env vars.
        // Env vars appear indented under `environment:` and have the form
        // `KEY: value` where KEY is UPPER_SNAKE_CASE (e.g., DATABASE_URL).
        // Top-level YAML keys like `secrets:`, `services:` are lowercase
        // and are NOT env vars.
        if let Some(colon_pos) = trimmed.find(':') {
            let key = &trimmed[..colon_pos];
            // UPPER_SNAKE_CASE: non-empty, all uppercase ASCII letters,
            // digits, or underscores, with at least one letter.
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && key.chars().any(|c| c.is_ascii_uppercase())
            {
                return true;
            }
        }
        false
    }

    /// Check if a value contains a URL with embedded credentials
    /// (e.g., `postgres://user:pass@host/db`).
    fn has_url_credentials(value: &str) -> bool {
        // Look for scheme://...:...@ pattern
        if let Some(scheme_end) = value.find("://") {
            let after_scheme = &value[scheme_end + 3..];
            // Must have user:pass@host pattern
            if let Some(at_pos) = after_scheme.find('@') {
                let userinfo = &after_scheme[..at_pos];
                return userinfo.contains(':');
            }
        }
        false
    }

    /// Scrub URL-embedded credentials: replace `user:pass` with
    /// `<REDACTED>:<REDACTED>` in `scheme://user:pass@host` patterns.
    fn scrub_url_credentials(value: &str) -> String {
        if let Some(scheme_end) = value.find("://") {
            let scheme = &value[..scheme_end + 3];
            let after_scheme = &value[scheme_end + 3..];
            if let Some(at_pos) = after_scheme.find('@') {
                let after_at = &after_scheme[at_pos..];
                return format!("{scheme}<REDACTED>:<REDACTED>{after_at}");
            }
        }
        value.to_string()
    }

    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Preserve top-level compose YAML structure keys.
        // These are unindented (or minimally indented) keys like `secrets:`,
        // `file:`, `external:` that are compose structure, not env vars.
        let indent_len = line.len() - line.trim_start().len();
        let upper = trimmed.to_uppercase();

        // Check for secret pattern match on env-var-style lines only.
        let is_env_assignment = is_env_var_line(trimmed);
        let matches_pattern = SECRET_PATTERNS.iter().any(|pat| upper.contains(pat));

        // Split env-var lines into (key_with_sep, value). Supports both
        // `KEY=value` and `KEY: value` (colon-space mapping style).
        let kv_split = if is_env_assignment {
            if let Some(eq_pos) = trimmed.find('=') {
                // KEY=value — separator is `=`, included in key portion
                Some((&trimmed[..eq_pos + 1], &trimmed[eq_pos + 1..]))
            } else if let Some(colon_pos) = trimmed.find(':') {
                // KEY: value — separator is `: `, included in key portion
                let rest = &trimmed[colon_pos + 1..];
                let value = rest.strip_prefix(' ').unwrap_or(rest);
                let sep_end = colon_pos + 1 + (rest.len() - value.len());
                Some((&trimmed[..sep_end], value))
            } else {
                None
            }
        } else {
            None
        };

        if is_env_assignment && matches_pattern {
            if let Some((key, value)) = kv_split {
                let indent = &line[..indent_len];
                // Check for URL-embedded credentials in the value
                if has_url_credentials(value) {
                    result.push_str(indent);
                    result.push_str(key);
                    result.push_str(&scrub_url_credentials(value));
                    result.push('\n');
                } else {
                    result.push_str(indent);
                    result.push_str(key);
                    result.push_str("<REDACTED>");
                    result.push('\n');
                }
            } else {
                result.push_str(line);
                result.push('\n');
            }
        } else if is_env_assignment && !matches_pattern {
            // Not a secret-named key, but check for URL-embedded credentials
            // in the value (e.g., DATABASE_URL without matching a pattern name
            // but containing postgres://user:pass@host).
            if let Some((key, value)) = kv_split {
                if has_url_credentials(value) {
                    let indent = &line[..indent_len];
                    result.push_str(indent);
                    result.push_str(key);
                    result.push_str(&scrub_url_credentials(value));
                    result.push('\n');
                } else {
                    result.push_str(line);
                    result.push('\n');
                }
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

    #[test]
    fn scrub_compose_secrets_redacts_url_embedded_credentials() {
        let input = "    DATABASE_URL=postgres://admin:s3cret@db.example.com/mydb\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(
            scrubbed.contains("<REDACTED>:<REDACTED>@db.example.com"),
            "URL-embedded credentials should be scrubbed: {scrubbed}"
        );
        assert!(!scrubbed.contains("admin"));
        assert!(!scrubbed.contains("s3cret"));
    }

    #[test]
    fn scrub_compose_secrets_redacts_redis_url() {
        let input = "    REDIS_URL=redis://user:pass@redis.internal:6379\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(!scrubbed.contains("pass"));
        assert!(scrubbed.contains("<REDACTED>:<REDACTED>@redis.internal"));
    }

    #[test]
    fn scrub_compose_secrets_preserves_secrets_block() {
        let input = "secrets:\n  db_password:\n    file: ./secrets/db_pass.txt\n  api_key:\n    external: true\n";
        let scrubbed = scrub_compose_secrets(input);
        assert_eq!(
            input, scrubbed,
            "top-level secrets: block must be preserved: {scrubbed}"
        );
    }

    #[test]
    fn scrub_compose_secrets_preserves_file_references() {
        let input = "    file: ./secrets/my_secret.txt\n";
        let scrubbed = scrub_compose_secrets(input);
        assert_eq!(input, scrubbed, "file: references must be preserved");
    }

    #[test]
    fn scrub_url_without_credentials_preserved() {
        let input = "    APP_URL=https://app.example.com/api\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(
            scrubbed.contains("https://app.example.com/api"),
            "URL without credentials should not be modified"
        );
    }

    #[test]
    fn scrub_compose_secrets_colon_style_url_credentials() {
        let input = "      DATABASE_URL: postgres://user:pass@db/prod\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(
            scrubbed.contains("<REDACTED>:<REDACTED>@db/prod"),
            "colon-style URL credentials should be scrubbed: {scrubbed}"
        );
        assert!(!scrubbed.contains("user"));
        assert!(!scrubbed.contains("pass"));
    }

    #[test]
    fn scrub_compose_secrets_eq_style_url_credentials_still_works() {
        let input = "      DATABASE_URL=postgres://user:pass@db/prod\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(
            scrubbed.contains("<REDACTED>:<REDACTED>@db/prod"),
            "equals-style URL credentials should still be scrubbed: {scrubbed}"
        );
        assert!(!scrubbed.contains("user"));
        assert!(!scrubbed.contains("pass"));
    }

    #[test]
    fn scrub_compose_secrets_colon_style_secret_pattern() {
        let input = "      DB_PASSWORD: hunter2\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(
            scrubbed.contains("DB_PASSWORD: <REDACTED>"),
            "colon-style secret pattern should be redacted: {scrubbed}"
        );
        assert!(!scrubbed.contains("hunter2"));
    }

    #[test]
    fn scrub_compose_secrets_colon_style_preserves_nonsecret() {
        let input = "      APP_PORT: 8080\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(
            scrubbed.contains("APP_PORT: 8080"),
            "colon-style non-secret should be preserved: {scrubbed}"
        );
    }
}
