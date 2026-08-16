use inspectah_core::traits::executor::Executor;
use std::collections::HashSet;

/// `rpm --queryformat` template that emits exactly one owned path per
/// line and nothing else.
///
/// `--dump` was used here previously. It also puts the path first, but
/// follows it with space-separated trailing fields, and it quotes
/// nothing: a path containing a space is indistinguishable from a path
/// followed by fields. Stock packages ship such paths (firmware blobs
/// named after hardware, for one), so every one of them fell out of the
/// owned set and the /usr walk reported it as unmanaged. Parsing from
/// the right does not rescue it either, because the trailing symlink
/// target field can itself contain spaces.
///
/// The argv is passed to `Command` unshelled, so the `\n` here is the
/// two-character escape `rpm` expands, not a literal newline.
const OWNED_PATHS_QUERY_FORMAT: &str = "[%{FILENAMES}\\n]";

/// Build the set of all file paths owned by any installed RPM package.
///
/// Queries every installed package for its file list, one path per
/// line. The returned set contains normalized absolute paths (no
/// trailing slashes, no double slashes).
///
/// This is intentionally separate from `RpmState.owned_paths` (which
/// is filtered to `/etc` for config deduplication). The /usr walk
/// needs the full unfiltered set to diff against.
///
/// Reused by Task 3 (extended /etc walk).
pub fn build_rpm_owned_set(exec: &dyn Executor) -> HashSet<String> {
    let result = exec.run(
        "rpm",
        &["--query", "--all", "--qf", OWNED_PATHS_QUERY_FORMAT],
    );
    let mut owned = HashSet::new();
    if result.exit_code != 0 {
        return owned;
    }
    for line in result.stdout.lines() {
        // The whole line is the path. Do not split on whitespace and do
        // not trim: both would corrupt paths that legally contain spaces.
        let normalized = normalize_path(line);
        if !normalized.is_empty() {
            owned.insert(normalized);
        }
    }
    owned
}

fn normalize_path(path: &str) -> String {
    path.trim_end_matches('/').replace("//", "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;

    /// The `MockExecutor` key is `cmd + " " + args.join(" ")`, and a
    /// mismatch silently yields exit 127 rather than a failure the test
    /// can see. Build it from the same constant the collector uses.
    fn owned_set_command_key() -> String {
        format!("rpm --query --all --qf {}", OWNED_PATHS_QUERY_FORMAT)
    }

    fn mock_owned_set(stdout: &str) -> MockExecutor {
        MockExecutor::new().with_command(
            &owned_set_command_key(),
            ExecResult {
                stdout: stdout.to_string(),
                exit_code: 0,
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_build_rpm_owned_set_parses_one_path_per_line() {
        let output = "/usr/bin/bash\n/usr/lib64/libc.so.6\n/etc/passwd\n";
        let exec = mock_owned_set(output);

        let owned = build_rpm_owned_set(&exec);
        assert!(owned.contains("/usr/bin/bash"));
        assert!(owned.contains("/usr/lib64/libc.so.6"));
        assert!(owned.contains("/etc/passwd"));
        assert_eq!(owned.len(), 3);
    }

    /// Regression: stock packages own paths containing spaces, and they
    /// must land in the owned set intact or the /usr walk reports them
    /// as unmanaged content the user is asked to act on.
    ///
    /// This path is real. `brcmfmac-firmware` owns it on RHEL 10, where
    /// it and 32 siblings were reported as actionable unmanaged /usr
    /// entries. Under the previous `--dump` parsing the line read:
    ///
    /// ```text
    /// /usr/lib/firmware/brcm/brcmfmac43241b4-sdio.Intel Corp.-VALLEYVIEW C0 PLATFORM.txt.xz 968 1769731200 fc6bc7e9860d2344e6ea45c6fff2e4eef9c14fe28107fad841f28d99b9cc66ee 0100644 root root 0 0 0 X
    /// ```
    ///
    /// and the first whitespace-delimited field truncated it at
    /// `...-sdio.Intel`.
    #[test]
    fn test_build_rpm_owned_set_keeps_paths_containing_spaces() {
        let firmware_path =
            "/usr/lib/firmware/brcm/brcmfmac43241b4-sdio.Intel Corp.-VALLEYVIEW C0 PLATFORM.txt.xz";
        let output = format!("/usr/bin/bash\n{}\n", firmware_path);
        let exec = mock_owned_set(&output);

        let owned = build_rpm_owned_set(&exec);
        assert!(
            owned.contains(firmware_path),
            "owned set lost the space-containing path; got {:?}",
            owned
        );
        assert!(
            !owned.contains("/usr/lib/firmware/brcm/brcmfmac43241b4-sdio.Intel"),
            "owned set contains a truncated path; got {:?}",
            owned
        );
        assert_eq!(owned.len(), 2);
    }

    #[test]
    fn test_build_rpm_owned_set_returns_empty_on_failure() {
        let exec = MockExecutor::new().with_command(
            &owned_set_command_key(),
            ExecResult {
                exit_code: 1,
                stderr: "rpm: not found".to_string(),
                ..Default::default()
            },
        );

        let owned = build_rpm_owned_set(&exec);
        assert!(owned.is_empty());
    }

    #[test]
    fn test_normalize_path_strips_trailing_slash() {
        assert_eq!(normalize_path("/usr/bin/"), "/usr/bin");
    }

    #[test]
    fn test_normalize_path_collapses_double_slashes() {
        assert_eq!(normalize_path("/usr//lib64//foo"), "/usr/lib64/foo");
    }

    #[test]
    fn test_normalize_path_noop_for_clean_path() {
        assert_eq!(normalize_path("/usr/bin/bash"), "/usr/bin/bash");
    }
}
