//! `inspectah version` subcommand — prints version, commit, and build date.
//!
//! The `INSPECTAH_COMMIT` and `INSPECTAH_DATE` env vars are set by
//! `crates/cli/build.rs` at compile time.

pub fn print_version() {
    println!(
        "inspectah {} (commit {}, built {})",
        env!("CARGO_PKG_VERSION"),
        env!("INSPECTAH_COMMIT"),
        env!("INSPECTAH_DATE"),
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn compile_time_vars_are_set() {
        let commit = env!("INSPECTAH_COMMIT");
        let date = env!("INSPECTAH_DATE");
        assert!(!commit.is_empty(), "INSPECTAH_COMMIT must not be empty");
        assert!(!date.is_empty(), "INSPECTAH_DATE must not be empty");
        assert_ne!(commit, "unknown", "build.rs must set commit hash");
        assert_ne!(date, "unknown", "build.rs must set build date");
    }
}
