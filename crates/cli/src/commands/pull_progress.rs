//! Pull progress display helpers.
//!
//! Handles TTY viewport rendering and non-TTY passthrough for
//! podman pull stderr output. All rendering functions are pure —
//! they take typed inputs and return formatted strings or side-effect
//! through provided writers.

use std::io::Write;

/// Minimum terminal width for TTY viewport. Below this, fall back to non-TTY.
pub const MIN_VIEWPORT_WIDTH: usize = 40;

/// Maximum viewport content width.
const MAX_VIEWPORT_WIDTH: usize = 72;

/// Dynamic viewport height: 30% of terminal rows, floor 8, cap 16.
pub fn viewport_height(terminal_rows: usize) -> usize {
    let height = (terminal_rows as f64 * 0.3).round() as usize;
    height.clamp(8, 16)
}

/// Strip ANSI escape sequences from a string.
///
/// Removes CSI sequences (\x1b[...X) and OSC sequences (\x1b]...\x07).
/// Used to clean podman's colored/cursor-controlled stderr output for
/// the TTY viewport.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            continue;
        }
        if c == '\x1b' {
            // CSI: \x1b[ ... <letter>
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() || next == 'H' || next == 'J' || next == 'K' {
                        break;
                    }
                }
            }
            // OSC: \x1b] ... \x07
            else if chars.peek() == Some(&']') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == '\x07' {
                        break;
                    }
                }
            }
            // Other escape — skip next char
            else {
                chars.next();
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Count unique completed blob transfers from podman pull stderr lines.
///
/// Looks for lines matching `Copying blob <sha256:...> done|skipped`.
/// Uses a HashSet on the sha256 prefix to deduplicate — podman may
/// emit multiple progress lines for the same blob before the final
/// `done`/`skipped` line.
///
/// Returns `None` if no completed blob lines were found (unexpected
/// output format or non-pull command). The count is best-effort
/// display-only; it is not persisted in the snapshot.
pub fn count_completed_blobs(stderr_lines: &[String]) -> Option<usize> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for line in stderr_lines {
        let stripped = strip_ansi(line);
        if !stripped.contains("Copying blob") {
            continue;
        }
        if !stripped.ends_with("done") && !stripped.ends_with("skipped") {
            continue;
        }
        // Extract the blob identifier (sha256:... prefix)
        if let Some(start) = stripped.find("sha256:") {
            let rest = &stripped[start..];
            let id = rest.split_whitespace().next().unwrap_or(rest);
            seen.insert(id.to_string());
        }
    }
    if seen.is_empty() {
        None
    } else {
        Some(seen.len())
    }
}

/// Format the pull summary line shown after pull completes.
///
/// Uses "blob transfers" rather than "layers" — the pull progress is
/// transport-level blob chatter, not stable image-model truth.
pub fn pull_summary_line(image_ref: &str, digest: &str, blob_count: Option<usize>) -> String {
    let short_digest = if digest.len() > 19 {
        &digest[..19]
    } else {
        digest
    };
    match blob_count {
        Some(n) => format!("Pulled {image_ref} ({n} blob transfers, {short_digest})"),
        None => format!("Pulled {image_ref} ({short_digest})"),
    }
}

/// Truncate a string to `max_len` characters, appending `...` if truncated.
fn truncate_line(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 1 {
        format!("{}…", &s[..max_len - 1])
    } else {
        "…".to_string()
    }
}

/// Create the non-TTY callback: prints each stderr line with `  pull: ` prefix.
///
/// Displayed output is sanitized to redact credentials. The raw
/// ANSI-stripped (but unsanitized) line is collected for post-completion
/// classification by `pull_failure::classify_pull_failure`.
pub fn non_tty_callback<'a>(
    collected: &'a mut Vec<String>,
    output: &'a mut dyn Write,
) -> impl FnMut(&str) + 'a {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if !cleaned.trim().is_empty() {
            let sanitized = super::pull_failure::sanitize_stderr(&cleaned);
            let _ = writeln!(output, "  pull: {sanitized}");
        }
        collected.push(cleaned);
    }
}

/// Create the TTY viewport callback: renders a dynamic-height box-drawing viewport
/// with recent stderr lines.
///
/// The `output` writer receives the viewport rendering (box-drawing, cursor
/// control). Content lines are sanitized before display. The raw
/// ANSI-stripped (but unsanitized) line is collected for classification.
pub fn tty_viewport_callback<'a>(
    collected: &'a mut Vec<String>,
    ring: &'a mut [String],
    ring_pos: &'a mut usize,
    content_width: usize,
    output: &'a mut dyn Write,
) -> impl FnMut(&str) + 'a {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if cleaned.trim().is_empty() {
            return;
        }
        collected.push(cleaned.clone());

        let viewport_lines = ring.len();

        // Sanitize before putting into ring buffer for display
        let sanitized = super::pull_failure::sanitize_stderr(&cleaned);
        ring[*ring_pos % viewport_lines] = truncate_line(&sanitized, content_width);
        *ring_pos += 1;

        // Move cursor up to clear previous viewport (if not first draw)
        if *ring_pos > 1 {
            // up (viewport_lines + 2) lines (top border + N content + bottom border)
            let _ = write!(output, "\x1b[{}A", viewport_lines + 2);
        }

        // Draw top border
        let _ = writeln!(
            output,
            "  \u{250c}{}\u{2510}",
            "\u{2500}".repeat(content_width + 2)
        );
        // Draw content lines
        for i in 0..viewport_lines {
            let idx = if *ring_pos >= viewport_lines {
                (*ring_pos - viewport_lines + i) % viewport_lines
            } else if i < *ring_pos {
                i
            } else {
                // Empty slot
                let _ = writeln!(
                    output,
                    "  \u{2502} {:<width$} \u{2502}",
                    "",
                    width = content_width
                );
                continue;
            };
            let _ = writeln!(
                output,
                "  \u{2502} {:<width$} \u{2502}",
                ring[idx],
                width = content_width
            );
        }
        // Draw bottom border
        let _ = writeln!(
            output,
            "  \u{2514}{}\u{2518}",
            "\u{2500}".repeat(content_width + 2)
        );
        let _ = output.flush();
    }
}

/// Clear the TTY viewport after pull completes (or fails).
///
/// Moves cursor up and clears each viewport line.
pub fn viewport_cleanup(viewport_lines: usize) {
    let stderr = std::io::stderr();
    let mut w = stderr.lock();
    // Clear (viewport_lines + 2) lines: top border + N content + bottom border
    let total_lines = viewport_lines + 2;
    let _ = write!(w, "\x1b[{}A", total_lines); // move up
    for _ in 0..total_lines {
        let _ = writeln!(w, "\x1b[2K"); // clear line, move down
    }
    let _ = write!(w, "\x1b[{}A", total_lines); // move back up
    let _ = w.flush();
}

/// Determine the viewport content width from terminal width.
pub fn viewport_content_width(term_width: usize) -> usize {
    // Box borders: 2 (left "  | ") + 2 (right " |") = 6 chars overhead
    let max = term_width.saturating_sub(6);
    max.min(MAX_VIEWPORT_WIDTH - 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_color() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn strip_ansi_cursor_movement() {
        assert_eq!(strip_ansi("\x1b[3Atext"), "text");
    }

    #[test]
    fn strip_ansi_no_escape() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn strip_ansi_carriage_return() {
        assert_eq!(strip_ansi("progress\rdone"), "progressdone");
    }

    #[test]
    fn count_blobs_normal() {
        let lines: Vec<String> = vec![
            "Copying blob sha256:aaa111 done".into(),
            "Copying blob sha256:bbb222 done".into(),
            "Copying blob sha256:ccc333 skipped".into(),
        ];
        assert_eq!(count_completed_blobs(&lines), Some(3));
    }

    #[test]
    fn count_blobs_deduplicates() {
        let lines: Vec<String> = vec![
            "Copying blob sha256:aaa111 42 MiB / 89 MiB".into(),
            "Copying blob sha256:aaa111 done".into(),
            "Copying blob sha256:aaa111 done".into(), // duplicate final
        ];
        assert_eq!(count_completed_blobs(&lines), Some(1));
    }

    #[test]
    fn count_blobs_with_progress_lines() {
        let lines: Vec<String> = vec![
            "Copying blob sha256:aaa111 42 MiB / 89 MiB".into(),
            "Copying blob sha256:aaa111 done".into(),
        ];
        assert_eq!(count_completed_blobs(&lines), Some(1));
    }

    #[test]
    fn count_blobs_empty() {
        let lines: Vec<String> = vec!["Writing manifest".into()];
        assert_eq!(count_completed_blobs(&lines), None);
    }

    #[test]
    fn count_blobs_with_ansi() {
        let lines: Vec<String> = vec!["\x1b[32mCopying blob sha256:aaa111 done\x1b[0m".into()];
        assert_eq!(count_completed_blobs(&lines), Some(1));
    }

    #[test]
    fn pull_summary_with_blobs() {
        let line = pull_summary_line("quay.io/test:latest", "sha256:abc123def456789", Some(7));
        assert!(line.contains("7 blob transfers"));
        // First 19 chars of digest: "sha256:abc123def45"
        assert!(line.contains("sha256:abc123def45"));
        assert!(!line.contains("layers"));
    }

    #[test]
    fn pull_summary_without_blobs() {
        let line = pull_summary_line("quay.io/test:latest", "sha256:abc123", None);
        assert!(!line.contains("blob transfers"));
        assert!(line.contains("sha256:abc123"));
    }

    #[test]
    fn truncate_line_short() {
        assert_eq!(truncate_line("hello", 10), "hello");
    }

    #[test]
    fn truncate_line_exact() {
        assert_eq!(truncate_line("hello", 5), "hello");
    }

    #[test]
    fn truncate_line_long() {
        let result = truncate_line("hello world", 6);
        assert!(result.ends_with('\u{2026}'));
        assert_eq!(result.chars().count(), 6);
    }

    #[test]
    fn viewport_content_width_normal() {
        // 80 col terminal: 80 - 6 = 74, capped at 66 (MAX_VIEWPORT_WIDTH - 6)
        let w = viewport_content_width(80);
        assert!(w <= MAX_VIEWPORT_WIDTH - 6);
        assert!(w > 0);
    }

    #[test]
    fn viewport_content_width_narrow() {
        let w = viewport_content_width(45);
        assert!(w > 0);
    }

    #[test]
    fn non_tty_callback_collects() {
        let mut collected = Vec::new();
        let mut buf = Vec::new();
        {
            let mut cb = non_tty_callback(&mut collected, &mut buf);
            cb("Copying blob sha256:aaa done");
            cb("Copying blob sha256:bbb skipped");
        }
        assert_eq!(collected.len(), 2);
        assert!(collected[0].contains("aaa"));
        let output = String::from_utf8_lossy(&buf);
        assert!(output.contains("pull:"));
    }

    #[test]
    fn non_tty_callback_sanitizes_displayed_output() {
        let mut collected = Vec::new();
        let mut buf = Vec::new();
        {
            let mut cb = non_tty_callback(&mut collected, &mut buf);
            cb("Error: Bearer eyJhbGciOiJSUzI1NiJ9.payload.sig unauthorized");
        }
        let output = String::from_utf8_lossy(&buf);
        // Displayed output should be sanitized
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("eyJhbGciOiJSUzI1NiJ9"));
        // Collected line should be raw (unsanitized) for classification
        assert!(collected[0].contains("eyJhbGciOiJSUzI1NiJ9"));
    }

    #[test]
    fn tty_viewport_callback_sanitizes_displayed_output() {
        let mut collected = Vec::new();
        let mut ring: Vec<String> = (0..3).map(|_| String::new()).collect();
        let mut ring_pos: usize = 0;
        let mut buf = Vec::new();
        {
            let mut cb =
                tty_viewport_callback(&mut collected, &mut ring, &mut ring_pos, 60, &mut buf);
            cb("failed with Bearer eyJsecrettoken rest of line");
        }
        let output = String::from_utf8_lossy(&buf);
        // Viewport display should be sanitized
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("eyJsecrettoken"));
        // Ring buffer should contain sanitized content
        assert!(ring[0].contains("[REDACTED]"));
        // Collected should be raw for classification
        assert!(collected[0].contains("eyJsecrettoken"));
    }

    #[test]
    fn viewport_height_scales_with_terminal() {
        assert_eq!(viewport_height(80), 16); // 80 * 0.3 = 24 -> capped at 16
        assert_eq!(viewport_height(50), 15); // 50 * 0.3 = 15
        assert_eq!(viewport_height(24), 8); // 24 * 0.3 = 7.2 -> floored at 8
        assert_eq!(viewport_height(10), 8); // 10 * 0.3 = 3 -> floored at 8
        assert_eq!(viewport_height(40), 12); // 40 * 0.3 = 12
    }
}
