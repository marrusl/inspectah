use base64::Engine;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::subscription::{
    SubscriptionFile, SubscriptionSection, match_entitlement_pairs,
};
use inspectah_core::types::warnings::Warning;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_SIZE: u64 = 1_048_576; // 1 MB safety valve

const ENTITLEMENT_DIR: &str = "/etc/pki/entitlement";
const CONSUMER_CERT: &str = "/etc/pki/consumer/cert.pem";
const RHSM_CONF: &str = "/etc/rhsm/rhsm.conf";
const RHSM_CA_DIR: &str = "/etc/rhsm/ca";
const REDHAT_REPO: &str = "/etc/yum.repos.d/redhat.repo";

/// Approved subscription roots -- symlinks must resolve within one of these.
const APPROVED_ROOTS: &[&str] = &[
    "/etc/pki/entitlement",
    "/etc/rhsm",
    "/etc/yum.repos.d/redhat.repo",
];

/// Inspects RHSM subscription material: entitlement certs, CA certs,
/// rhsm.conf, redhat.repo, and consumer cert metadata.
pub struct SubscriptionInspector;

impl SubscriptionInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SubscriptionInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for SubscriptionInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Subscription
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;
        let mut section = SubscriptionSection::default();
        let mut warnings = Vec::new();

        // Populate source hostname for fleet provenance.
        // InspectionContext has no hostname field -- read /etc/hostname via executor,
        // matching how collect.rs populates snapshot.meta["hostname"] (line 211).
        section.source_hostname = exec
            .read_file(Path::new(exec.host_root()).join("etc/hostname").as_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // 1. Entitlement certs
        collect_dir_pems(
            exec,
            ENTITLEMENT_DIR,
            &mut section.entitlement_certs,
            &mut warnings,
        );

        // 2. RHSM config
        if let Some(f) = collect_single_file(exec, RHSM_CONF, &mut warnings) {
            section.config_files.push(f);
        }

        // 3. CA certs
        collect_dir_pems(exec, RHSM_CA_DIR, &mut section.ca_certs, &mut warnings);

        // 4. redhat.repo
        if let Some(f) = collect_single_file(exec, REDHAT_REPO, &mut warnings) {
            section.config_files.push(f);
        }

        // 5. Parse org metadata from consumer cert (metadata only, not collected)
        parse_org_metadata(exec, CONSUMER_CERT, &mut section);

        // 6. Parse cert expiry from entitlement certs
        parse_cert_expiries(&mut section);

        // 7. Evaluate bundle completeness with serial-number matching
        evaluate_bundle_completeness(&mut section, &mut warnings);

        Ok(InspectorOutput {
            section: SectionData::Subscription(section),
            warnings,
            redaction_hints: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// Warning helper
// ---------------------------------------------------------------------------

fn warn(message: impl Into<String>) -> Warning {
    Warning {
        inspector: "subscription".into(),
        message: message.into(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Symlink boundary check
// ---------------------------------------------------------------------------

/// Normalize a path by resolving `.` and `..` components lexically (no filesystem access).
/// This is used for symlink boundary validation in unit tests where the filesystem
/// paths don't exist. For real execution the real executor's read_link already
/// follows the chain.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop unless we'd go above root
                if !components.is_empty() {
                    components.pop();
                }
            }
            Component::CurDir => {} // skip
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Check whether a symlink target resolves within an approved subscription root.
/// `link_target` is the raw string from `read_link()`. `file_path` is the
/// absolute path of the symlink (within host_root). `host_root` is the
/// executor's host root prefix.
fn is_symlink_within_approved_roots(link_target: &str, file_path: &Path, host_root: &Path) -> bool {
    let target_path = Path::new(link_target);

    // Resolve relative symlinks against the symlink's parent directory
    let resolved = if target_path.is_absolute() {
        host_root.join(link_target.trim_start_matches('/'))
    } else {
        let parent = file_path.parent().unwrap_or(file_path);
        parent.join(target_path)
    };

    let canonical = normalize_path(&resolved);

    APPROVED_ROOTS.iter().any(|root| {
        let full_root = host_root.join(root.trim_start_matches('/'));
        canonical.starts_with(&full_root)
    })
}

// ---------------------------------------------------------------------------
// Collection helpers
// ---------------------------------------------------------------------------

fn collect_dir_pems(
    exec: &dyn Executor,
    dir: &str,
    dest: &mut Vec<SubscriptionFile>,
    warnings: &mut Vec<Warning>,
) {
    let dir_path = Path::new(exec.host_root()).join(dir.trim_start_matches('/'));
    let entries = match exec.read_dir(&dir_path) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return; // optional directory, silently skip
            }
            warnings.push(warn(format!("Cannot read {dir}: {e}")));
            return;
        }
    };

    for entry in &entries {
        if !entry.ends_with(".pem") {
            continue;
        }
        let file_path = dir_path.join(entry);

        // Validate symlink stays within approved subscription roots.
        if let Ok(target) = exec.read_link(&file_path)
            && !is_symlink_within_approved_roots(&target, &file_path, exec.host_root())
        {
            warnings.push(warn(format!(
                "Symlink {dir}/{entry} resolves outside \
                 approved subscription paths, skipped"
            )));
            continue;
        }

        match exec.read_file(&file_path) {
            Ok(content) => {
                let size = content.len() as u64;
                if size > MAX_FILE_SIZE {
                    warnings.push(warn(format!(
                        "{dir}/{entry}: file exceeds 1 MB limit ({size} bytes), skipped"
                    )));
                    continue;
                }
                let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
                dest.push(SubscriptionFile {
                    path: format!("{dir}/{entry}"),
                    content: encoded,
                    size_bytes: size,
                    cert_expiry: None, // filled by parse_cert_expiries
                });
            }
            Err(e) => {
                warnings.push(warn(format!("Cannot read {dir}/{entry}: {e}")));
            }
        }
    }
}

fn collect_single_file(
    exec: &dyn Executor,
    path: &str,
    warnings: &mut Vec<Warning>,
) -> Option<SubscriptionFile> {
    let file_path = Path::new(exec.host_root()).join(path.trim_start_matches('/'));

    // Validate symlink boundary
    if let Ok(target) = exec.read_link(&file_path)
        && !is_symlink_within_approved_roots(&target, &file_path, exec.host_root())
    {
        warnings.push(warn(format!(
            "{path} is a symlink resolving outside \
             approved subscription paths, skipped"
        )));
        return None;
    }

    match exec.read_file(&file_path) {
        Ok(content) => {
            let size = content.len() as u64;
            if size > MAX_FILE_SIZE {
                warnings.push(warn(format!(
                    "{path}: file exceeds 1 MB limit ({size} bytes), skipped"
                )));
                return None;
            }
            let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
            Some(SubscriptionFile {
                path: path.into(),
                content: encoded,
                size_bytes: size,
                cert_expiry: None,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            warnings.push(warn(format!("Cannot read {path}: {e}")));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Cert parsing
// ---------------------------------------------------------------------------

fn parse_org_metadata(
    exec: &dyn Executor,
    consumer_cert_path: &str,
    section: &mut SubscriptionSection,
) {
    let file_path = Path::new(exec.host_root()).join(consumer_cert_path.trim_start_matches('/'));
    let content = match exec.read_file(&file_path) {
        Ok(c) => c,
        Err(_) => return, // consumer cert missing is fine -- not build-required
    };

    if let Some(der) = pem_to_der(&content)
        && let Ok((_, cert)) = x509_parser::parse_x509_certificate(&der)
    {
        // Extract org_id from subject O= field
        if let Some(attr) = cert.subject().iter_organization().next()
            && let Ok(val) = attr.attr_value().as_str()
        {
            section.org_id = Some(val.to_string());
        }
        // Extract system_uuid from subject CN= field
        if let Some(attr) = cert.subject().iter_common_name().next()
            && let Ok(val) = attr.attr_value().as_str()
        {
            section.system_uuid = Some(val.to_string());
        }
        // Extract rhsm_server from issuer O= field
        if let Some(attr) = cert.issuer().iter_organization().next()
            && let Ok(val) = attr.attr_value().as_str()
        {
            section.rhsm_server = Some(val.to_string());
        }
    }
}

/// Parse cert expiry using typed `time::OffsetDateTime`.
fn parse_cert_expiries(section: &mut SubscriptionSection) {
    let mut earliest: Option<time::OffsetDateTime> = None;

    for cert_file in &mut section.entitlement_certs {
        if cert_file.path.ends_with("-key.pem") {
            continue;
        }
        let raw = match base64::engine::general_purpose::STANDARD.decode(&cert_file.content) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let pem_str = match std::str::from_utf8(&raw) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let der = match pem_to_der(pem_str) {
            Some(d) => d,
            None => continue,
        };
        let cert = match x509_parser::parse_x509_certificate(&der) {
            Ok((_, c)) => c,
            Err(_) => continue,
        };

        // ASN1Time exposes timestamp() which returns i64 epoch seconds
        let not_after = cert.validity().not_after;
        if let Ok(expiry) = time::OffsetDateTime::from_unix_timestamp(not_after.timestamp()) {
            cert_file.cert_expiry = Some(expiry);
            match &earliest {
                None => earliest = Some(expiry),
                Some(e) if expiry < *e => earliest = Some(expiry),
                _ => {}
            }
        }
    }

    section.earliest_expiry = earliest;
}

// ---------------------------------------------------------------------------
// Bundle completeness
// ---------------------------------------------------------------------------

/// Bundle completeness evaluated using serial-number-matched EntitlementPair.
fn evaluate_bundle_completeness(section: &mut SubscriptionSection, warnings: &mut Vec<Warning>) {
    let mut missing = Vec::new();

    // Check entitlement cert+key pairs by serial number
    let (pairs, orphans) = match_entitlement_pairs(&section.entitlement_certs);
    if pairs.is_empty() {
        missing.push("entitlement cert+key pair (matched by serial number)");
    }
    for orphan in &orphans {
        warnings.push(warn(format!(
            "Entitlement file has no matching pair: {orphan}"
        )));
    }

    // Check rhsm.conf
    let has_rhsm_conf = section
        .config_files
        .iter()
        .any(|f| f.path.contains("rhsm.conf"));
    if !has_rhsm_conf {
        missing.push("rhsm.conf");
    }

    // Check CA certs
    if section.ca_certs.is_empty() {
        missing.push("CA certs from /etc/rhsm/ca/");
    }

    // Check redhat.repo
    let has_redhat_repo = section
        .config_files
        .iter()
        .any(|f| f.path.contains("redhat.repo"));
    if !has_redhat_repo {
        missing.push("redhat.repo");
    }

    if !missing.is_empty() {
        section.incomplete = true;
        for item in &missing {
            warnings.push(warn(format!(
                "Incomplete subscription bundle: missing {item}"
            )));
        }
    }
}

// ---------------------------------------------------------------------------
// PEM helper
// ---------------------------------------------------------------------------

fn pem_to_der(pem_content: &str) -> Option<Vec<u8>> {
    let begin = pem_content.find("-----BEGIN CERTIFICATE-----")?;
    let end = pem_content.find("-----END CERTIFICATE-----")?;
    let b64_start = begin + "-----BEGIN CERTIFICATE-----".len();
    let b64 = &pem_content[b64_start..end].replace(['\n', '\r', ' '], "");
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::Executor;

    #[test]
    fn test_collects_entitlement_certs() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["123.pem", "123-key.pem"])
            .with_file("/etc/pki/entitlement/123.pem", "cert-content")
            .with_file("/etc/pki/entitlement/123-key.pem", "key-content");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);

        assert_eq!(certs.len(), 2);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_missing_entitlement_dir_skipped_silently() {
        let exec = MockExecutor::new();
        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_permission_denied_produces_warning() {
        let exec = MockExecutor::new()
            .with_dir_error("/etc/pki/entitlement", std::io::ErrorKind::PermissionDenied);

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("Cannot read"));
    }

    #[test]
    fn test_file_over_1mb_rejected() {
        let big = "x".repeat(1_048_577);
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["big.pem"])
            .with_file("/etc/pki/entitlement/big.pem", &big);

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("1 MB"));
    }

    #[test]
    fn test_symlink_outside_subscription_roots_rejected() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["evil.pem"])
            .with_link("/etc/pki/entitlement/evil.pem", "/etc/shadow");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("outside"));
    }

    #[test]
    fn test_symlink_within_subscription_root_accepted() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["good.pem"])
            .with_link(
                "/etc/pki/entitlement/good.pem",
                "/etc/pki/entitlement/real.pem",
            )
            .with_file("/etc/pki/entitlement/good.pem", "cert-content");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert_eq!(certs.len(), 1);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_symlink_dotdot_escape_rejected() {
        let exec = MockExecutor::new()
            .with_dir("/etc/pki/entitlement", vec!["escape.pem"])
            .with_link("/etc/pki/entitlement/escape.pem", "../../shadow");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert!(certs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("outside"));
    }

    #[test]
    fn test_single_file_symlink_outside_roots_rejected() {
        let exec = MockExecutor::new()
            .with_link("/etc/rhsm/rhsm.conf", "/etc/shadow")
            .with_file("/etc/rhsm/rhsm.conf", "content");

        let mut warnings = Vec::new();
        let result = collect_single_file(&exec, RHSM_CONF, &mut warnings);
        assert!(result.is_none());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("outside"));
    }

    #[test]
    fn test_single_file_symlink_within_roots_accepted() {
        let exec = MockExecutor::new()
            .with_link("/etc/rhsm/rhsm.conf", "/etc/rhsm/rhsm.conf.real")
            .with_file("/etc/rhsm/rhsm.conf", "[rhsm]");

        let mut warnings = Vec::new();
        let result = collect_single_file(&exec, RHSM_CONF, &mut warnings);
        assert!(result.is_some());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_bundle_completeness_all_present_serial_matched() {
        let mut section = SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123.pem".into(),
                    content: "c".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123-key.pem".into(),
                    content: "k".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
            ],
            ca_certs: vec![SubscriptionFile {
                path: "/etc/rhsm/ca/redhat-uep.pem".into(),
                content: "ca".into(),
                size_bytes: 1,
                cert_expiry: None,
            }],
            config_files: vec![
                SubscriptionFile {
                    path: "/etc/rhsm/rhsm.conf".into(),
                    content: "cfg".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/yum.repos.d/redhat.repo".into(),
                    content: "repo".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        evaluate_bundle_completeness(&mut section, &mut warnings);
        assert!(!section.incomplete);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_bundle_incomplete_mismatched_serials() {
        let mut section = SubscriptionSection {
            entitlement_certs: vec![
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
            ],
            ca_certs: vec![SubscriptionFile {
                path: "ca".into(),
                content: "ca".into(),
                size_bytes: 1,
                cert_expiry: None,
            }],
            config_files: vec![
                SubscriptionFile {
                    path: "/etc/rhsm/rhsm.conf".into(),
                    content: "cfg".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "redhat.repo".into(),
                    content: "r".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        evaluate_bundle_completeness(&mut section, &mut warnings);
        assert!(section.incomplete);
        // Should have orphan warnings AND missing pair warning
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("no matching pair"))
        );
    }

    #[test]
    fn test_bundle_incomplete_missing_redhat_repo() {
        let mut section = SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile {
                    path: "123.pem".into(),
                    content: "c".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "123-key.pem".into(),
                    content: "k".into(),
                    size_bytes: 1,
                    cert_expiry: None,
                },
            ],
            ca_certs: vec![SubscriptionFile {
                path: "ca".into(),
                content: "ca".into(),
                size_bytes: 1,
                cert_expiry: None,
            }],
            config_files: vec![SubscriptionFile {
                path: "/etc/rhsm/rhsm.conf".into(),
                content: "cfg".into(),
                size_bytes: 1,
                cert_expiry: None,
            }],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        evaluate_bundle_completeness(&mut section, &mut warnings);
        assert!(section.incomplete);
        assert!(warnings.iter().any(|w| w.message.contains("redhat.repo")));
    }

    #[test]
    fn test_collects_redhat_repo() {
        let exec = MockExecutor::new().with_file(
            "/etc/yum.repos.d/redhat.repo",
            "[rhel-base]\nbaseurl=https://cdn",
        );
        let mut warnings = Vec::new();
        let result = collect_single_file(&exec, REDHAT_REPO, &mut warnings);
        assert!(result.is_some());
        let f = result.expect("should have collected redhat.repo");
        assert_eq!(f.path, REDHAT_REPO);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_missing_redhat_repo_returns_none() {
        let exec = MockExecutor::new();
        let mut warnings = Vec::new();
        let result = collect_single_file(&exec, REDHAT_REPO, &mut warnings);
        assert!(result.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_non_pem_files_skipped() {
        let exec = MockExecutor::new()
            .with_dir(
                "/etc/pki/entitlement",
                vec!["123.pem", "readme.txt", "notes.md"],
            )
            .with_file("/etc/pki/entitlement/123.pem", "cert");

        let mut certs = Vec::new();
        let mut warnings = Vec::new();
        collect_dir_pems(&exec, ENTITLEMENT_DIR, &mut certs, &mut warnings);
        assert_eq!(certs.len(), 1);
    }

    #[test]
    fn test_hostname_populated() {
        let exec = MockExecutor::new().with_file("/etc/hostname", "test-host.example.com\n");

        let hostname = exec
            .read_file(Path::new("/etc/hostname"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        assert_eq!(hostname, Some("test-host.example.com".to_string()));
    }

    #[test]
    fn test_normalize_path_removes_dotdot() {
        let p = normalize_path(Path::new("/etc/pki/entitlement/../../shadow"));
        assert_eq!(p, PathBuf::from("/etc/shadow"));
    }

    #[test]
    fn test_normalize_path_preserves_clean() {
        let p = normalize_path(Path::new("/etc/pki/entitlement/123.pem"));
        assert_eq!(p, PathBuf::from("/etc/pki/entitlement/123.pem"));
    }

    #[test]
    fn test_is_symlink_within_approved_roots_absolute() {
        let result = is_symlink_within_approved_roots(
            "/etc/pki/entitlement/real.pem",
            Path::new("/etc/pki/entitlement/link.pem"),
            Path::new("/"),
        );
        assert!(result);
    }

    #[test]
    fn test_is_symlink_outside_approved_roots() {
        let result = is_symlink_within_approved_roots(
            "/etc/shadow",
            Path::new("/etc/pki/entitlement/evil.pem"),
            Path::new("/"),
        );
        assert!(!result);
    }

    #[test]
    fn test_is_symlink_relative_escape() {
        let result = is_symlink_within_approved_roots(
            "../../shadow",
            Path::new("/etc/pki/entitlement/escape.pem"),
            Path::new("/"),
        );
        assert!(!result);
    }

    #[test]
    fn test_pem_to_der_valid() {
        // Minimal valid base64 between PEM markers
        let pem = "-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----";
        let der = pem_to_der(pem);
        assert!(der.is_some());
        assert_eq!(der.expect("should decode"), vec![0u8]);
    }

    #[test]
    fn test_pem_to_der_no_markers() {
        let result = pem_to_der("not a PEM file");
        assert!(result.is_none());
    }

    #[test]
    fn test_bundle_completeness_empty_section() {
        let mut section = SubscriptionSection::default();
        let mut warnings = Vec::new();
        evaluate_bundle_completeness(&mut section, &mut warnings);
        assert!(section.incomplete);
        // Should warn about missing entitlement pair, rhsm.conf, CA certs, redhat.repo
        assert!(warnings.len() >= 4);
    }
}
