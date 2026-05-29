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
pub fn detect_ambient_subscription() -> AmbientSubscription {
    let passthrough_marker = Path::new("/usr/share/rhel/secrets/etc-pki-entitlement");
    if !passthrough_marker.exists() {
        return AmbientSubscription::NotAvailable;
    }

    let mut missing = Vec::new();

    // 1. Serial-matched entitlement cert+key pair.
    let ent_dir = Path::new("/etc/pki/entitlement");
    if !ent_dir.exists() {
        missing.push("/etc/pki/entitlement directory");
    } else {
        let has_matched_pair = check_serial_matched_pair(ent_dir);
        if !has_matched_pair {
            missing.push("serial-matched entitlement cert+key pair");
        }
    }

    // 2. rhsm.conf.
    if !Path::new("/etc/rhsm/rhsm.conf").exists() {
        missing.push("rhsm.conf");
    }

    // 3. CA certs.
    let ca_dir = Path::new("/etc/rhsm/ca");
    let has_ca = ca_dir.exists()
        && std::fs::read_dir(ca_dir)
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
    if !Path::new("/etc/yum.repos.d/redhat.repo").exists() {
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
}
