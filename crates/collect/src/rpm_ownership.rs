use inspectah_core::traits::executor::Executor;
use std::collections::HashSet;

/// Build the set of all file paths owned by any installed RPM package.
///
/// Uses `rpm --query --all --dump` which lists every file from every
/// installed package, one per line, with the path as the first
/// whitespace-delimited field. The returned set contains normalized
/// absolute paths (no trailing slashes, no double slashes).
///
/// This is intentionally separate from `RpmState.owned_paths` (which
/// is filtered to `/etc` for config deduplication). The /usr walk
/// needs the full unfiltered set to diff against.
///
/// Reused by Task 3 (extended /etc walk).
pub fn build_rpm_owned_set(exec: &dyn Executor) -> HashSet<String> {
    let result = exec.run("rpm", &["--query", "--all", "--dump"]);
    let mut owned = HashSet::new();
    if result.exit_code != 0 {
        return owned;
    }
    for line in result.stdout.lines() {
        if let Some(path) = line.split_whitespace().next() {
            let normalized = normalize_path(path);
            if !normalized.is_empty() {
                owned.insert(normalized);
            }
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

    #[test]
    fn test_build_rpm_owned_set_parses_dump() {
        let dump_output = "/usr/bin/bash 1234567 abcdef0123456789 0100755 root root 0 0 0 X\n\
                           /usr/lib64/libc.so.6 9876543 fedcba9876543210 0100755 root root 0 0 0 X\n\
                           /etc/passwd 512 1234 0100644 root root 0 1 0 X\n";
        let exec = MockExecutor::new().with_command(
            "rpm --query --all --dump",
            ExecResult {
                stdout: dump_output.to_string(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let owned = build_rpm_owned_set(&exec);
        assert!(owned.contains("/usr/bin/bash"));
        assert!(owned.contains("/usr/lib64/libc.so.6"));
        assert!(owned.contains("/etc/passwd"));
        assert_eq!(owned.len(), 3);
    }

    #[test]
    fn test_build_rpm_owned_set_returns_empty_on_failure() {
        let exec = MockExecutor::new().with_command(
            "rpm --query --all --dump",
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
