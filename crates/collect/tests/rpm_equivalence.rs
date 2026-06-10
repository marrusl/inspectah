//! Epoch normalization contract tests.
//!
//! Proves that the shell parser normalizes epoch values correctly:
//! "(none)" becomes "0", numeric epochs are preserved as strings.

use inspectah_core::types::rpm::PackageEntry;

/// Shell path: parse NEVRA line, epoch "(none)" → "0"
fn shell_path_entry(
    epoch: &str,
    name: &str,
    version: &str,
    release: &str,
    arch: &str,
) -> PackageEntry {
    use inspectah_collect::inspectors::rpm::parser::parse_nevra;
    let line = format!("{epoch}:{name}-{version}-{release}.{arch}");
    parse_nevra(&line).expect("parse failed")
}

/// Construct the expected PackageEntry with a numeric epoch string.
fn expected_entry(
    epoch_num: u64,
    name: &str,
    version: &str,
    release: &str,
    arch: &str,
) -> PackageEntry {
    let epoch = epoch_num.to_string();
    PackageEntry {
        name: name.into(),
        epoch,
        version: version.into(),
        release: release.into(),
        arch: arch.into(),
        ..Default::default()
    }
}

#[test]
fn test_epoch_zero_normalization_matches() {
    let shell = shell_path_entry("(none)", "bash", "5.2.26", "3.el9", "x86_64");
    let expected = expected_entry(0, "bash", "5.2.26", "3.el9", "x86_64");
    assert_eq!(
        shell.epoch, expected.epoch,
        "epoch must match: shell={}, expected={}",
        shell.epoch, expected.epoch
    );
    assert_eq!(shell.name, expected.name);
    assert_eq!(shell.version, expected.version);
    assert_eq!(shell.release, expected.release);
    assert_eq!(shell.arch, expected.arch);
}

#[test]
fn test_epoch_nonzero_normalization_matches() {
    let shell = shell_path_entry("2", "openssl", "3.0.7", "1.el9", "x86_64");
    let expected = expected_entry(2, "openssl", "3.0.7", "1.el9", "x86_64");
    assert_eq!(shell.epoch, expected.epoch);
    assert_eq!(shell.name, expected.name);
}

#[test]
fn test_full_entry_shape_matches() {
    let shell = shell_path_entry("0", "httpd", "2.4.57", "5.el9", "x86_64");
    let expected = expected_entry(0, "httpd", "2.4.57", "5.el9", "x86_64");
    assert_eq!(shell.name, expected.name);
    assert_eq!(shell.epoch, expected.epoch);
    assert_eq!(shell.version, expected.version);
    assert_eq!(shell.release, expected.release);
    assert_eq!(shell.arch, expected.arch);
}
