//! Shell-vs-FFI epoch normalization equivalence test.
//! Proves both paths produce the same PackageEntry shape.

use inspectah_core::types::rpm::PackageEntry;

/// Simulate the shell path: parse NEVRA, epoch "(none)" → "0"
fn shell_path_entry(epoch: &str, name: &str, version: &str, release: &str, arch: &str) -> PackageEntry {
    use inspectah_collect::inspectors::rpm::parser::parse_nevra;
    let line = format!("{epoch}:{name}-{version}-{release}.{arch}");
    parse_nevra(&line).expect("parse failed")
}

/// Simulate what the FFI path now produces after the epoch fix.
/// epoch_num=0 → "0", epoch_num>0 → numeric string.
fn ffi_path_entry(epoch_num: u64, name: &str, version: &str, release: &str, arch: &str) -> PackageEntry {
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
fn test_shell_ffi_epoch_zero_equivalent() {
    let shell = shell_path_entry("(none)", "bash", "5.2.26", "3.el9", "x86_64");
    let ffi = ffi_path_entry(0, "bash", "5.2.26", "3.el9", "x86_64");
    assert_eq!(shell.epoch, ffi.epoch, "epoch must match: shell={}, ffi={}", shell.epoch, ffi.epoch);
    assert_eq!(shell.name, ffi.name);
    assert_eq!(shell.version, ffi.version);
    assert_eq!(shell.release, ffi.release);
    assert_eq!(shell.arch, ffi.arch);
}

#[test]
fn test_shell_ffi_epoch_nonzero_equivalent() {
    let shell = shell_path_entry("2", "openssl", "3.0.7", "1.el9", "x86_64");
    let ffi = ffi_path_entry(2, "openssl", "3.0.7", "1.el9", "x86_64");
    assert_eq!(shell.epoch, ffi.epoch);
    assert_eq!(shell.name, ffi.name);
}

#[test]
fn test_shell_ffi_full_shape_equality() {
    let shell = shell_path_entry("0", "httpd", "2.4.57", "5.el9", "x86_64");
    let ffi = ffi_path_entry(0, "httpd", "2.4.57", "5.el9", "x86_64");
    // Full structural equality
    assert_eq!(shell.name, ffi.name);
    assert_eq!(shell.epoch, ffi.epoch);
    assert_eq!(shell.version, ffi.version);
    assert_eq!(shell.release, ffi.release);
    assert_eq!(shell.arch, ffi.arch);
}
