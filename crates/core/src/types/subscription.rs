use serde::{Deserialize, Serialize};

/// A single file collected from the subscription tree.
/// `cert_expiry` is a typed UTC timestamp — serialized as RFC 3339 at the serde boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionFile {
    pub path: String,
    pub content: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub cert_expiry: Option<time::OffsetDateTime>,
}

/// A cert/key pair matched by serial number.
/// Completeness requires BOTH cert and key for a given serial.
#[derive(Debug, Clone, PartialEq)]
pub struct EntitlementPair {
    pub serial: String,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
}

impl EntitlementPair {
    pub fn is_complete(&self) -> bool {
        self.cert_path.is_some() && self.key_path.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionSection {
    pub entitlement_certs: Vec<SubscriptionFile>,
    pub ca_certs: Vec<SubscriptionFile>,
    pub config_files: Vec<SubscriptionFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub earliest_expiry: Option<time::OffsetDateTime>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub incomplete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhsm_server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hostname: Option<String>,
}

/// Extract serial number from an entitlement cert/key filename.
/// Convention: `<serial>.pem` for certs, `<serial>-key.pem` for keys.
pub fn parse_serial(path: &str) -> Option<(String, bool)> {
    let filename = std::path::Path::new(path).file_name()?.to_str()?;
    if let Some(serial) = filename.strip_suffix("-key.pem") {
        Some((serial.to_string(), true))
    } else {
        filename
            .strip_suffix(".pem")
            .map(|serial| (serial.to_string(), false))
    }
}

/// Group entitlement files into serial-matched pairs.
/// Returns pairs and a list of orphaned files (cert without key or vice versa).
pub fn match_entitlement_pairs(files: &[SubscriptionFile]) -> (Vec<EntitlementPair>, Vec<String>) {
    use std::collections::BTreeMap;

    let mut pairs: BTreeMap<String, EntitlementPair> = BTreeMap::new();

    for f in files {
        if let Some((serial, is_key)) = parse_serial(&f.path) {
            let pair = pairs
                .entry(serial.clone())
                .or_insert_with(|| EntitlementPair {
                    serial,
                    cert_path: None,
                    key_path: None,
                });
            if is_key {
                pair.key_path = Some(f.path.clone());
            } else {
                pair.cert_path = Some(f.path.clone());
            }
        }
    }

    let mut orphans = Vec::new();
    let mut complete = Vec::new();
    for (_, pair) in pairs {
        if pair.is_complete() {
            complete.push(pair);
        } else {
            let path = pair
                .cert_path
                .as_ref()
                .or(pair.key_path.as_ref())
                .unwrap()
                .clone();
            orphans.push(path);
        }
    }

    (complete, orphans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_section_roundtrip() {
        let expiry = time::OffsetDateTime::from_unix_timestamp(1_723_680_000).unwrap();
        let section = SubscriptionSection {
            entitlement_certs: vec![SubscriptionFile {
                path: "/etc/pki/entitlement/123.pem".into(),
                content: "base64data".into(),
                size_bytes: 1024,
                cert_expiry: Some(expiry),
            }],
            ca_certs: vec![],
            config_files: vec![],
            earliest_expiry: Some(expiry),
            incomplete: false,
            org_id: Some("12345".into()),
            system_uuid: Some("abc-def-ghi".into()),
            rhsm_server: Some("subscription.rhsm.redhat.com".into()),
            source_hostname: Some("host-a.example.com".into()),
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: SubscriptionSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
        assert!(json.contains("2024-08-15T"));
        assert!(!json.contains("Mon,"));
    }

    #[test]
    fn test_subscription_section_default_is_empty() {
        let section = SubscriptionSection::default();
        assert!(section.entitlement_certs.is_empty());
        assert!(section.earliest_expiry.is_none());
        assert!(!section.incomplete);
        assert!(section.org_id.is_none());
        assert!(section.source_hostname.is_none());
    }

    #[test]
    fn test_subscription_section_skips_none_fields() {
        let section = SubscriptionSection::default();
        let json = serde_json::to_string(&section).unwrap();
        assert!(!json.contains("earliest_expiry"));
        assert!(!json.contains("org_id"));
        assert!(!json.contains("incomplete"));
        assert!(!json.contains("source_hostname"));
    }

    #[test]
    fn test_parse_serial_cert() {
        assert_eq!(
            parse_serial("/etc/pki/entitlement/123456.pem"),
            Some(("123456".into(), false))
        );
    }

    #[test]
    fn test_parse_serial_key() {
        assert_eq!(
            parse_serial("/etc/pki/entitlement/123456-key.pem"),
            Some(("123456".into(), true))
        );
    }

    #[test]
    fn test_match_pairs_complete() {
        let files = vec![
            SubscriptionFile {
                path: "/etc/pki/entitlement/111.pem".into(),
                content: "c".into(),
                size_bytes: 1,
                cert_expiry: None,
            },
            SubscriptionFile {
                path: "/etc/pki/entitlement/111-key.pem".into(),
                content: "k".into(),
                size_bytes: 1,
                cert_expiry: None,
            },
        ];
        let (pairs, orphans) = match_entitlement_pairs(&files);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].is_complete());
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_match_pairs_mismatched_serials() {
        let files = vec![
            SubscriptionFile {
                path: "/etc/pki/entitlement/111.pem".into(),
                content: "c".into(),
                size_bytes: 1,
                cert_expiry: None,
            },
            SubscriptionFile {
                path: "/etc/pki/entitlement/222-key.pem".into(),
                content: "k".into(),
                size_bytes: 1,
                cert_expiry: None,
            },
        ];
        let (pairs, orphans) = match_entitlement_pairs(&files);
        assert!(pairs.is_empty());
        assert_eq!(orphans.len(), 2);
    }

    #[test]
    fn test_match_pairs_missing_key() {
        let files = vec![SubscriptionFile {
            path: "/etc/pki/entitlement/111.pem".into(),
            content: "c".into(),
            size_bytes: 1,
            cert_expiry: None,
        }];
        let (pairs, orphans) = match_entitlement_pairs(&files);
        assert!(pairs.is_empty());
        assert_eq!(orphans.len(), 1);
    }
}
