//! Integration test for the librpm FFI wrapper.
//!
//! Only compiled when `ffi-rpm` is enabled — requires librpm-devel on the host.

#[cfg(feature = "ffi-rpm")]
#[test]
fn test_librpm_query_returns_packages() {
    use inspectah_collect::ffi::rpm::query_all_packages;
    let packages = query_all_packages().expect("librpm query failed");
    assert!(!packages.is_empty(), "host must have packages installed");
    assert!(
        packages.iter().any(|p| p.name == "bash"),
        "bash should be installed"
    );
}
