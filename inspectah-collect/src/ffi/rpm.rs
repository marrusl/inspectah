//! Safe wrapper around librpm for RPM database queries.
//!
//! This module is only compiled when the `ffi-rpm` feature is enabled.
//! It encapsulates all `unsafe` librpm FFI calls — no raw pointers cross
//! the public API boundary.
//!
//! # Usage
//!
//! ```ignore
//! use inspectah_collect::ffi::rpm::query_all_packages;
//! let packages = query_all_packages().expect("librpm query failed");
//! ```

#![cfg(feature = "ffi-rpm")]

use inspectah_core::types::rpm::PackageEntry;

/// Errors from the librpm FFI layer.
#[derive(Debug, thiserror::Error)]
pub enum RpmFfiError {
    /// librpm initialization (rpmReadConfigFiles) failed.
    #[error("librpm initialization failed")]
    InitFailed,

    /// An rpmdb query returned an error or produced invalid data.
    #[error("rpmdb query failed: {0}")]
    QueryFailed(String),
}

// ---------------------------------------------------------------------------
// Raw C bindings (private)
// ---------------------------------------------------------------------------
//
// These are the minimal librpm symbols we need. Keeping them in a private
// inner module ensures no raw pointers escape to callers.

mod sys {
    use libc::{c_char, c_int, c_void};

    // Opaque handles — we only ever hold pointers to these.
    pub enum RpmTs {}
    pub enum RpmDbMatchIterator {}
    pub enum Header {}

    // rpmTag values we use.
    pub const RPMTAG_NAME: c_int = 1000;
    pub const RPMTAG_EPOCH: c_int = 1003;
    pub const RPMTAG_VERSION: c_int = 1001;
    pub const RPMTAG_RELEASE: c_int = 1002;
    pub const RPMTAG_ARCH: c_int = 1022;

    // RPMDBI_PACKAGES = 0 (iterate all packages)
    pub const RPMDBI_PACKAGES: c_int = 0;

    extern "C" {
        pub fn rpmReadConfigFiles(file: *const c_char, target: *const c_char) -> c_int;
        pub fn rpmtsCreate() -> *mut RpmTs;
        pub fn rpmtsFree(ts: *mut RpmTs) -> *mut RpmTs;
        pub fn rpmtsInitIterator(
            ts: *const RpmTs,
            rpmtag: c_int,
            keyp: *const c_void,
            keylen: libc::size_t,
        ) -> *mut RpmDbMatchIterator;
        pub fn rpmdbNextIterator(mi: *mut RpmDbMatchIterator) -> *mut Header;
        pub fn rpmdbFreeIterator(mi: *mut RpmDbMatchIterator) -> *mut RpmDbMatchIterator;
        pub fn headerGetString(h: *mut Header, tag: c_int) -> *const c_char;
        pub fn headerGetNumber(h: *mut Header, tag: c_int) -> u64;
    }
}

/// Query all installed RPM packages from the local rpmdb.
///
/// Returns a `Vec<PackageEntry>` with `name`, `epoch`, `version`, `release`,
/// and `arch` populated. The `state` field is left at its default (`Added`)
/// — classification happens in a later pipeline stage.
///
/// # Errors
///
/// Returns `RpmFfiError::InitFailed` if librpm configuration cannot be read,
/// or `RpmFfiError::QueryFailed` if the database iterator cannot be created.
pub fn query_all_packages() -> Result<Vec<PackageEntry>, RpmFfiError> {
    use std::ffi::CStr;
    use std::ptr;

    // Initialize librpm (idempotent in practice).
    let rc = unsafe { sys::rpmReadConfigFiles(ptr::null(), ptr::null()) };
    if rc != 0 {
        return Err(RpmFfiError::InitFailed);
    }

    // Create a transaction set.
    let ts = unsafe { sys::rpmtsCreate() };
    if ts.is_null() {
        return Err(RpmFfiError::QueryFailed(
            "rpmtsCreate returned null".into(),
        ));
    }

    // Open an iterator over all installed packages.
    let mi = unsafe {
        sys::rpmtsInitIterator(ts, sys::RPMDBI_PACKAGES, ptr::null(), 0)
    };
    if mi.is_null() {
        unsafe { sys::rpmtsFree(ts) };
        return Err(RpmFfiError::QueryFailed(
            "rpmtsInitIterator returned null".into(),
        ));
    }

    let mut packages = Vec::new();

    loop {
        let header = unsafe { sys::rpmdbNextIterator(mi) };
        if header.is_null() {
            break;
        }

        let name = unsafe { header_get_string(header, sys::RPMTAG_NAME) };
        let version = unsafe { header_get_string(header, sys::RPMTAG_VERSION) };
        let release = unsafe { header_get_string(header, sys::RPMTAG_RELEASE) };
        let arch = unsafe { header_get_string(header, sys::RPMTAG_ARCH) };
        let epoch_num = unsafe { sys::headerGetNumber(header, sys::RPMTAG_EPOCH) };

        let epoch = if epoch_num == 0 {
            "(none)".to_string()
        } else {
            epoch_num.to_string()
        };

        // Skip gpg-pubkey pseudo-packages — they have no arch.
        if name == "gpg-pubkey" {
            continue;
        }

        packages.push(PackageEntry {
            name,
            epoch,
            version,
            release,
            arch,
            ..Default::default()
        });
    }

    // Clean up.
    unsafe {
        sys::rpmdbFreeIterator(mi);
        sys::rpmtsFree(ts);
    }

    Ok(packages)
}

/// Extract a string tag from a header, returning an owned `String`.
///
/// # Safety
///
/// `header` must be a valid, non-null pointer obtained from `rpmdbNextIterator`.
unsafe fn header_get_string(header: *mut sys::Header, tag: libc::c_int) -> String {
    let ptr = sys::headerGetString(header, tag);
    if ptr.is_null() {
        return String::new();
    }
    CStr::from_ptr(ptr)
        .to_string_lossy()
        .into_owned()
}
