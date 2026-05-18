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

/// Number of recent lines shown in the viewport.
const VIEWPORT_LINES: usize = 3;

/// Strip ANSI escape sequences from a string.
///
/// Removes CSI sequences (\x1b[...X) and OSC sequences (\x1b]...\x07).
/// Used to clean podman's colored/cursor-controlled stderr output for
/// the TTY viewport.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
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
/// Also collects lines for post-completion blob counting.
pub fn non_tty_callback(collected: &mut Vec<String>) -> impl FnMut(&str) + '_ {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if !cleaned.trim().is_empty() {
            eprintln!("  pull: {cleaned}");
        }
        collected.push(cleaned);
    }
}

/// Create the TTY viewport callback: renders a 3-line box-drawing viewport
/// with recent stderr lines.
///
/// Also collects lines for post-completion blob counting.
pub fn tty_viewport_callback<'a>(
    collected: &'a mut Vec<String>,
    ring: &'a mut [String; VIEWPORT_LINES],
    ring_pos: &'a mut usize,
    content_width: usize,
) -> impl FnMut(&str) + 'a {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if cleaned.trim().is_empty() {
            return;
        }
        collected.push(cleaned.clone());

        // Push into ring buffer
        ring[*ring_pos % VIEWPORT_LINES] = truncate_line(&cleaned, content_width);
        *ring_pos += 1;

        // Redraw viewport
        let stderr = std::io::stderr();
        let mut w = stderr.lock();

        // Move cursor up to clear previous viewport (if not first draw)
        if *ring_pos > 1 {
            let _ = write!(w, "\x1b[5A"); // up 5 lines (top border + 3 content + bottom border)
        }

        // Draw top border
        let _ = writeln!(
            w,
            "  \u{250c}{}\u{2510}",
            "\u{2500}".repeat(content_width + 2)
        );
        // Draw content lines
        for i in 0..VIEWPORT_LINES {
            let idx = if *ring_pos >= VIEWPORT_LINES {
                (*ring_pos - VIEWPORT_LINES + i) % VIEWPORT_LINES
            } else if i < *ring_pos {
                i
            } else {
                // Empty slot
                let _ = writeln!(
                    w,
                    "  \u{2502} {:<width$} \u{2502}",
                    "",
                    width = content_width
                );
                continue;
            };
            let _ = writeln!(
                w,
                "  \u{2502} {:<width$} \u{2502}",
                ring[idx],
                width = content_width
            );
        }
        // Draw bottom border
        let _ = writeln!(
            w,
            "  \u{2514}{}\u{2518}",
            "\u{2500}".repeat(content_width + 2)
        );
        let _ = w.flush();
    }
}

/// Clear the TTY viewport after pull completes (or fails).
///
/// Moves cursor up and clears each viewport line.
pub fn viewport_cleanup() {
    let stderr = std::io::stderr();
    let mut w = stderr.lock();
    // Clear 5 lines: top border + 3 content + bottom border
    let _ = write!(w, "\x1b[5A"); // move up 5
    for _ in 0..5 {
        let _ = writeln!(w, "\x1b[2K"); // clear line, move down
    }
    let _ = write!(w, "\x1b[5A"); // move back up
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
        {
            let mut cb = non_tty_callback(&mut collected);
            cb("Copying blob sha256:aaa done");
            cb("Copying blob sha256:bbb skipped");
        }
        assert_eq!(collected.len(), 2);
        assert!(collected[0].contains("aaa"));
    }
}
