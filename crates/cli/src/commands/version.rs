//! `inspectah version` subcommand: prints the version line.
//!
//! `INSPECTAH_VERSION` is set by `crates/cli/build.rs` at compile time: the
//! bare package version for a build of its release tag, `<version>+g<hash>`
//! for any other release build, and `<version>+dev` for debug builds.

/// The line `inspectah version` prints. `inspectah --version` prints the
/// same line, because clap joins the command name and `INSPECTAH_VERSION`.
pub const VERSION_LINE: &str = concat!("inspectah ", env!("INSPECTAH_VERSION"));

pub fn print_version() {
    println!("{VERSION_LINE}");
}

#[cfg(test)]
mod tests {
    use super::VERSION_LINE;
    use clap::CommandFactory;

    #[test]
    fn version_line_starts_with_the_package_version() {
        let version = env!("INSPECTAH_VERSION");
        assert!(
            version.starts_with(env!("CARGO_PKG_VERSION")),
            "INSPECTAH_VERSION {version} must start with the package version"
        );
        let rest = &version[env!("CARGO_PKG_VERSION").len()..];
        assert!(
            rest.is_empty() || rest.starts_with('+'),
            "anything after the version must be semver build metadata, got {rest:?}"
        );
    }

    #[test]
    fn dash_dash_version_prints_the_same_line() {
        assert_eq!(
            crate::Cli::command().render_version(),
            format!("{VERSION_LINE}\n")
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn dev_builds_carry_fixed_version_metadata() {
        assert_eq!(
            env!("INSPECTAH_VERSION"),
            concat!(env!("CARGO_PKG_VERSION"), "+dev")
        );
    }
}
