//! `inspectah version` subcommand — prints version, commit, and build date.

pub fn print_version() {
    let version = env!("CARGO_PKG_VERSION");
    // Commit and date are populated by build-time env vars when available.
    // During local development, these default to "unknown".
    let commit = option_env!("INSPECTAH_COMMIT").unwrap_or("unknown");
    let date = option_env!("INSPECTAH_DATE").unwrap_or("unknown");

    println!("inspectah {version}");
    println!("commit: {commit}");
    println!("date:   {date}");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_version_string_not_empty() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(!version.is_empty());
    }
}
