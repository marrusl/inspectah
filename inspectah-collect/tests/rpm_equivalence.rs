//! Epoch normalization contract tests.
//!
//! Proves that the shell parser and the FFI epoch normalization produce
//! identical PackageEntry shapes. This does NOT exercise the real FFI
//! path (query_all_packages) — that requires librpm on Linux.
//!
//! Real FFI proof: enable `ffi-rpm` feature and run on a Linux host
//! with librpm-devel installed. See tests/ffi_rpm.rs for that path.

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

/// What the FFI path produces after the epoch fix: epoch_num → numeric string.
fn ffi_epoch_normalized(
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
    let ffi = ffi_epoch_normalized(0, "bash", "5.2.26", "3.el9", "x86_64");
    assert_eq!(
        shell.epoch, ffi.epoch,
        "epoch must match: shell={}, ffi={}",
        shell.epoch, ffi.epoch
    );
    assert_eq!(shell.name, ffi.name);
    assert_eq!(shell.version, ffi.version);
    assert_eq!(shell.release, ffi.release);
    assert_eq!(shell.arch, ffi.arch);
}

#[test]
fn test_epoch_nonzero_normalization_matches() {
    let shell = shell_path_entry("2", "openssl", "3.0.7", "1.el9", "x86_64");
    let ffi = ffi_epoch_normalized(2, "openssl", "3.0.7", "1.el9", "x86_64");
    assert_eq!(shell.epoch, ffi.epoch);
    assert_eq!(shell.name, ffi.name);
}

#[test]
fn test_full_entry_shape_matches() {
    let shell = shell_path_entry("0", "httpd", "2.4.57", "5.el9", "x86_64");
    let ffi = ffi_epoch_normalized(0, "httpd", "2.4.57", "5.el9", "x86_64");
    assert_eq!(shell.name, ffi.name);
    assert_eq!(shell.epoch, ffi.epoch);
    assert_eq!(shell.version, ffi.version);
    assert_eq!(shell.release, ffi.release);
    assert_eq!(shell.arch, ffi.arch);
}
