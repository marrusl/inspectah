//! The refine baseline banner's counts, from a scanned host.

use crate::unit_hosts::{UnitHost, scan, session};
use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};

/// The scan removes base-image packages from the added list and records
/// them in `baseline_suppressed`; the banner's "in base image" count is
/// that list's length, not the base matches left among refine's packages,
/// which on a scanned host are none. The host's own package is counted as
/// user-installed, not as a base match.
#[test]
fn the_baseline_banner_counts_the_packages_the_scan_matched_to_the_base_image() {
    let mut snap = scan(
        UnitHost::new()
            .base_package("glibc")
            .base_package("openssl")
            .package("httpd"),
    );
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.8".into(),
        strategy: ResolutionStrategy::OsRelease,
    });
    let suppressed = snap
        .rpm
        .as_ref()
        .and_then(|rpm| rpm.baseline_suppressed.clone())
        .expect("the scan recorded a baseline");
    assert!(
        suppressed.contains(&"glibc.x86_64".to_owned())
            && suppressed.contains(&"openssl.x86_64".to_owned()),
        "{suppressed:?}"
    );

    let summary = session(&snap)
        .baseline_summary()
        .expect("a baseline summary");
    assert_eq!(summary.baseline_count, suppressed.len());
    assert!(summary.baseline_count >= 2, "{summary:?}");
}
