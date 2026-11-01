use std::ops::Range;

/// How a key and its value are written on a single line.
///
/// Every hand-rolled scrubber in this codebase shared one defect: it
/// found the right *line* and then took the wrong *span*, because it
/// searched for the separator from the start of the line rather than
/// from the end of the key it had already matched. There were eight
/// instances across six functions. [`value_span_after_key`] is the one
/// place that decision is now made, and these are the four syntaxes its
/// callers actually parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSyntax {
    /// `"key": "value"` — a JSON object member. A colon, then a
    /// double-quoted run. The span excludes the quotes.
    JsonMember,
    /// `source "value"` or `source 'value'` — a quoted argument after
    /// whitespace, in either quote style. The span excludes the quotes,
    /// and the closing quote must match the opening one.
    QuotedArgument,
    /// `--index-url VALUE` or `--index-url=VALUE` — an option argument
    /// separated by whitespace or `=`, running to the next whitespace.
    OptionArgument,
    /// `KEY=value` or `KEY: value` — an assignment whose value runs to
    /// the end of the line.
    Assignment,
}

/// Byte offset of the first non-whitespace character of `s`.
fn leading_whitespace(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

/// The span inside a quoted run that begins at the first non-whitespace
/// character at or after `from`. The closing quote must match the
/// opening one, so an apostrophe inside a double-quoted value, or a
/// quoted comment later on the line, cannot end the run.
fn quoted_run(line: &str, from: usize, quotes: &[char]) -> Option<Range<usize>> {
    let rest = line.get(from..)?;
    let open = from + leading_whitespace(rest);
    let quote = line.get(open..)?.chars().next()?;
    if !quotes.contains(&quote) {
        return None;
    }
    let inner = open + quote.len_utf8();
    let close = line.get(inner..)?.find(quote)?;
    Some(inner..inner + close)
}

/// Locate the value belonging to the key that ends at `key_end`.
///
/// `key_end` is the byte offset just past the key the caller already
/// matched. The caller knows which key it routed this line for, and
/// this function must not re-guess it: searching forward from the key
/// is the entire point. A bare `line.find(':')` locates the key-value
/// separator only when the matched key happens to be the first one on
/// the line, which is how a compact `"publishConfig": { "registry":
/// "https://user:pass@host" }` kept its credential.
///
/// Returns the byte range of the value within `line`, excluding quotes.
///
/// # A `None` is not "nothing to scrub"
///
/// `None` means the value could not be located. The caller routed this
/// line here *because it matched a key*, so `None` says the line is
/// written in a form this function does not understand and its value
/// has gone unexamined. Callers must surface that rather than return
/// the line unchanged and say nothing — that silence is what let four
/// separate misses ship under a `FullyRedacted` label.
pub fn value_span_after_key(
    line: &str,
    key_end: usize,
    syntax: ValueSyntax,
) -> Option<Range<usize>> {
    if key_end > line.len() || !line.is_char_boundary(key_end) {
        return None;
    }
    let rest = &line[key_end..];

    match syntax {
        ValueSyntax::JsonMember => {
            let colon = rest.find(':')?;
            // Only whitespace may sit between the key and its colon. If
            // anything else does, the colon belongs to a different
            // member and following it would be the original defect.
            if !rest[..colon].trim().is_empty() {
                return None;
            }
            quoted_run(line, key_end + colon + 1, &['"'])
        }
        ValueSyntax::QuotedArgument => quoted_run(line, key_end, &['"', '\'']),
        ValueSyntax::OptionArgument => {
            // The character right after the key has to be the
            // separator, so a prefix match against a longer option does
            // not silently take that option's argument.
            let sep = rest.chars().next()?;
            if sep != '=' && !sep.is_whitespace() {
                return None;
            }
            let after = key_end + sep.len_utf8();
            let start = after + leading_whitespace(&line[after..]);
            if start >= line.len() {
                return None;
            }
            let end = line[start..]
                .find(char::is_whitespace)
                .map_or(line.len(), |p| start + p);
            Some(start..end)
        }
        ValueSyntax::Assignment => {
            let sep = rest.chars().next()?;
            if sep != '=' && sep != ':' {
                return None;
            }
            let after = key_end + sep.len_utf8();
            let start = after + leading_whitespace(&line[after..]);
            Some(start..line.len())
        }
    }
}

/// A line a scrubber was asked to handle and could not.
///
/// The scrubbers are rewriters, not detectors: when one could not
/// locate the value it was routed to rewrite, it returned the line
/// unchanged and emitted nothing, so the snapshot stayed
/// `FullyRedacted` over a live credential with no finding anywhere to
/// contradict it. This is that silence made reportable.
///
/// It deliberately carries no line text. These travel into
/// `snapshot.redactions`, which is serialized into the export, and the
/// whole premise is that the line may hold a credential nothing
/// managed to strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnscrubbedLine {
    /// 1-based line number within the scrubbed content.
    pub line: u32,
    /// The key or directive that routed this line to a scrubber.
    pub key: String,
}

/// Scrubbed content, plus the lines the scrubber could not handle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScrubOutcome {
    pub content: String,
    pub unscrubbed: Vec<UnscrubbedLine>,
}

impl ScrubOutcome {
    /// Content only, for the callers that have no way to report.
    pub fn into_content(self) -> String {
        self.content
    }
}

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

    /// The byte length of the assignment key at the start of `body`.
    ///
    /// The key is the leading run of identifier characters, and the
    /// character right after it must be the separator. Locating the key
    /// first is what fixes the old split: `kv_split` used to take
    /// `trimmed.find('=')`, so a colon-style line whose *value* carried
    /// an `=` — `DATABASE_URL: postgres://u:p@h/db?sslmode=require` is
    /// the measured case — split inside the query string, produced a
    /// "key" that was the whole credentialed URL, and emitted the line
    /// untouched.
    fn assignment_key_len(body: &str) -> Option<usize> {
        let len = body
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.' || *b == b'-')
            .count();
        if len == 0 || len == body.len() {
            return None;
        }
        let sep = body.as_bytes()[len];
        let key = &body[..len];
        match sep {
            // `KEY=value` is unambiguously an assignment in compose.
            b'=' => {}
            b':' => {
                // A colon is also YAML structure. Only an UPPER_SNAKE
                // key is an env var; `file:` under a `secrets:` block,
                // or `command:`, is not, and redacting those would
                // rewrite the compose file's own structure.
                if !key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                    || !key.bytes().any(|b| b.is_ascii_uppercase())
                {
                    return None;
                }
                // A bare `KEY:` with no value is a nested block header.
                if body[len + 1..].trim().is_empty() {
                    return None;
                }
            }
            _ => return None,
        }
        Some(len)
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

        let indent = &line[..line.len() - line.trim_start().len()];
        let upper = trimmed.to_uppercase();
        let matches_pattern = SECRET_PATTERNS.iter().any(|pat| upper.contains(pat));

        // A list-style entry (`- KEY=value`) carries its dash before the
        // key; everything after it is the assignment.
        let dash = if trimmed.starts_with("- ") { 2 } else { 0 };
        let body = &trimmed[dash..];

        // Locate the key, then the value that belongs to *that* key.
        let span = assignment_key_len(body)
            .and_then(|key_len| value_span_after_key(body, key_len, ValueSyntax::Assignment));

        // Not an assignment: ordinary YAML structure. `assignment_key_len`
        // verifies the separator itself, so once it answers there is no
        // "routed here but could not locate the value" state left for
        // this scrubber to be silent about.
        let Some(span) = span else {
            result.push_str(line);
            result.push('\n');
            continue;
        };

        let value = &body[span.clone()];
        let key_with_sep = &body[..span.start];

        // A secret-like key name redacts the whole value. Any key at all
        // redacts credentials embedded in a URL, which is how a
        // `DATABASE_URL` with no secret-like word in its name is caught.
        let replacement = if has_url_credentials(value) {
            Some(scrub_url_credentials(value))
        } else if matches_pattern {
            Some("<REDACTED>".to_string())
        } else {
            None
        };

        match replacement {
            Some(new_value) => {
                result.push_str(indent);
                result.push_str(&trimmed[..dash]);
                result.push_str(key_with_sep);
                result.push_str(&new_value);
                result.push('\n');
            }
            None => {
                result.push_str(line);
                result.push('\n');
            }
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
