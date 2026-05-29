//! RHEL pass-through detection with ambient subscription validation.
//!
//! Validates the ambient bundle against the same four-component contract
//! as scan-side `evaluate_bundle_completeness()`:
//! 1. Serial-matched entitlement cert+key pair
//! 2. rhsm.conf present
//! 3. At least one CA cert
//! 4. `/etc/yum.repos.d/redhat.repo` present (host-managed by subscription-manager)

use std::path::Path;

/// Result of RHEL ambient subscription detection.
#[derive(Debug, Clone, PartialEq)]
pub enum AmbientSubscription {
    /// RHEL host with valid ambient subscription.
    Available,
    /// RHEL host detected but ambient bundle is incomplete/invalid.
    IncompleteBundle { reason: String },
    /// Not a RHEL host (no pass-through path).
    NotAvailable,
}

/// Detect RHEL subscription pass-through and validate the ambient bundle.
///
/// Checks for `/usr/share/rhel/secrets/etc-pki-entitlement` and then validates
/// the ambient bundle against the same four-component contract as scan-side
/// `evaluate_bundle_completeness()`:
/// 1. Serial-matched entitlement cert+key pair
/// 2. rhsm.conf present
/// 3. At least one CA cert
/// 4. `/etc/yum.repos.d/redhat.repo` present (host-managed by subscription-manager)
///
/// Note: A successful ambient subscription assumes stock RHEL subscription-manager
/// pass-through behavior for container builds.
pub fn detect_ambient_subscription() -> AmbientSubscription {
    detect_ambient_subscription_in(Path::new("/"))
}

/// Internal helper: detect ambient subscription relative to a root path.
///
/// This allows tests to use temporary directories with controlled contents.
fn detect_ambient_subscription_in(root: &Path) -> AmbientSubscription {
    let passthrough_marker = root.join("usr/share/rhel/secrets/etc-pki-entitlement");
    if !passthrough_marker.exists() {
        return AmbientSubscription::NotAvailable;
    }

    let mut missing = Vec::new();

    // 1. Serial-matched entitlement cert+key pair.
    let ent_dir = root.join("etc/pki/entitlement");
    if !ent_dir.exists() {
        missing.push("/etc/pki/entitlement directory");
    } else {
        let has_matched_pair = check_serial_matched_pair(&ent_dir);
        if !has_matched_pair {
            missing.push("serial-matched entitlement cert+key pair");
        }
    }

    // 2. rhsm.conf.
    if !root.join("etc/rhsm/rhsm.conf").exists() {
        missing.push("rhsm.conf");
    }

    // 3. CA certs.
    let ca_dir = root.join("etc/rhsm/ca");
    let has_ca = ca_dir.exists()
        && std::fs::read_dir(&ca_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_string_lossy().ends_with(".pem"))
            })
            .unwrap_or(false);
    if !has_ca {
        missing.push("CA certs in /etc/rhsm/ca/");
    }

    // 4. redhat.repo -- host-managed by subscription-manager.
    if !root.join("etc/yum.repos.d/redhat.repo").exists() {
        missing.push("redhat.repo at /etc/yum.repos.d/redhat.repo");
    }

    if !missing.is_empty() {
        return AmbientSubscription::IncompleteBundle {
            reason: format!("missing: {}", missing.join(", ")),
        };
    }

    AmbientSubscription::Available
}

/// Check whether `dir` contains at least one serial-matched cert+key pair.
///
/// Convention: `<serial>.pem` for certs, `<serial>-key.pem` for keys.
fn check_serial_matched_pair(dir: &Path) -> bool {
    let mut serials: std::collections::BTreeMap<String, (bool, bool)> =
        std::collections::BTreeMap::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(serial) = name.strip_suffix("-key.pem") {
            serials.entry(serial.to_string()).or_default().1 = true;
        } else if let Some(serial) = name.strip_suffix(".pem") {
            serials.entry(serial.to_string()).or_default().0 = true;
        }
    }

    serials.values().any(|(cert, key)| *cert && *key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ambient_subscription_not_available_on_non_rhel() {
        // On dev machines (macOS, non-RHEL Linux), the pass-through marker
        // does not exist, so detection should return NotAvailable.
        let result = detect_ambient_subscription();
        // This test is environment-dependent. On RHEL it may pass differently.
        // On macOS/non-RHEL: NotAvailable is expected.
        assert!(
            matches!(
                result,
                AmbientSubscription::NotAvailable | AmbientSubscription::IncompleteBundle { .. }
            ),
            "on non-RHEL, expected NotAvailable or IncompleteBundle, got: {result:?}"
        );
    }

    #[test]
    fn test_check_serial_matched_pair_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!check_serial_matched_pair(tmp.path()));
    }

    #[test]
    fn test_check_serial_matched_pair_complete() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("12345.pem"), "cert").unwrap();
        std::fs::write(tmp.path().join("12345-key.pem"), "key").unwrap();
        assert!(check_serial_matched_pair(tmp.path()));
    }

    #[test]
    fn test_check_serial_matched_pair_cert_only() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("12345.pem"), "cert").unwrap();
        assert!(!check_serial_matched_pair(tmp.path()));
    }

    #[test]
    fn test_check_serial_matched_pair_key_only() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("12345-key.pem"), "key").unwrap();
        assert!(!check_serial_matched_pair(tmp.path()));
    }

    #[test]
    fn test_check_serial_matched_pair_mismatched() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("111.pem"), "cert").unwrap();
        std::fs::write(tmp.path().join("222-key.pem"), "key").unwrap();
        assert!(!check_serial_matched_pair(tmp.path()));
    }

    #[test]
    fn test_check_serial_matched_pair_nonexistent_dir() {
        let path = std::path::PathBuf::from("/tmp/nonexistent-entitlement-check-dir");
        assert!(!check_serial_matched_pair(&path));
    }

    #[test]
    fn test_ambient_enum_debug() {
        // Verify the enum variants are printable.
        let available = AmbientSubscription::Available;
        let incomplete = AmbientSubscription::IncompleteBundle {
            reason: "missing: rhsm.conf".into(),
        };
        let not_available = AmbientSubscription::NotAvailable;
        assert_eq!(format!("{available:?}"), "Available");
        assert!(format!("{incomplete:?}").contains("rhsm.conf"));
        assert_eq!(format!("{not_available:?}"), "NotAvailable");
    }

    /// Helper: create a complete ambient subscription bundle in a temp directory.
    fn setup_complete_ambient(root: &std::path::Path) {
        // Create passthrough marker.
        let marker = root.join("usr/share/rhel/secrets/etc-pki-entitlement");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();

        // Create entitlement pair.
        let ent_dir = root.join("etc/pki/entitlement");
        std::fs::create_dir_all(&ent_dir).unwrap();
        std::fs::write(ent_dir.join("12345.pem"), "cert").unwrap();
        std::fs::write(ent_dir.join("12345-key.pem"), "key").unwrap();

        // Create rhsm.conf.
        let rhsm_dir = root.join("etc/rhsm");
        std::fs::create_dir_all(&rhsm_dir).unwrap();
        std::fs::write(rhsm_dir.join("rhsm.conf"), "config").unwrap();

        // Create CA cert.
        let ca_dir = root.join("etc/rhsm/ca");
        std::fs::create_dir_all(&ca_dir).unwrap();
        std::fs::write(ca_dir.join("redhat-uep.pem"), "ca").unwrap();

        // Create redhat.repo.
        let repos_dir = root.join("etc/yum.repos.d");
        std::fs::create_dir_all(&repos_dir).unwrap();
        std::fs::write(repos_dir.join("redhat.repo"), "repo").unwrap();
    }

    #[test]
    fn test_detect_ambient_complete_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        setup_complete_ambient(tmp.path());
        let result = detect_ambient_subscription_in(tmp.path());
        assert_eq!(result, AmbientSubscription::Available);
    }

    #[test]
    fn test_detect_ambient_missing_entitlement_pair() {
        let tmp = tempfile::tempdir().unwrap();
        setup_complete_ambient(tmp.path());
        // Remove entitlement files.
        std::fs::remove_file(tmp.path().join("etc/pki/entitlement/12345.pem")).unwrap();
        std::fs::remove_file(tmp.path().join("etc/pki/entitlement/12345-key.pem")).unwrap();

        let result = detect_ambient_subscription_in(tmp.path());
        match result {
            AmbientSubscription::IncompleteBundle { reason } => {
                assert!(reason.contains("serial-matched entitlement cert+key pair"));
            }
            _ => panic!("expected IncompleteBundle, got: {result:?}"),
        }
    }

    #[test]
    fn test_detect_ambient_missing_rhsm_conf() {
        let tmp = tempfile::tempdir().unwrap();
        setup_complete_ambient(tmp.path());
        // Remove rhsm.conf.
        std::fs::remove_file(tmp.path().join("etc/rhsm/rhsm.conf")).unwrap();

        let result = detect_ambient_subscription_in(tmp.path());
        match result {
            AmbientSubscription::IncompleteBundle { reason } => {
                assert!(reason.contains("rhsm.conf"));
            }
            _ => panic!("expected IncompleteBundle, got: {result:?}"),
        }
    }

    #[test]
    fn test_detect_ambient_missing_ca_cert() {
        let tmp = tempfile::tempdir().unwrap();
        setup_complete_ambient(tmp.path());
        // Remove CA cert.
        std::fs::remove_file(tmp.path().join("etc/rhsm/ca/redhat-uep.pem")).unwrap();

        let result = detect_ambient_subscription_in(tmp.path());
        match result {
            AmbientSubscription::IncompleteBundle { reason } => {
                assert!(reason.contains("CA certs"));
            }
            _ => panic!("expected IncompleteBundle, got: {result:?}"),
        }
    }

    #[test]
    fn test_detect_ambient_missing_redhat_repo() {
        let tmp = tempfile::tempdir().unwrap();
        setup_complete_ambient(tmp.path());
        // Remove redhat.repo.
        std::fs::remove_file(tmp.path().join("etc/yum.repos.d/redhat.repo")).unwrap();

        let result = detect_ambient_subscription_in(tmp.path());
        match result {
            AmbientSubscription::IncompleteBundle { reason } => {
                assert!(reason.contains("redhat.repo"));
            }
            _ => panic!("expected IncompleteBundle, got: {result:?}"),
        }
    }
}
