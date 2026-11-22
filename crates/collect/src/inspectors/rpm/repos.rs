use super::dnf_vars::{DnfVars, ExpandError};
use inspectah_core::machine_state;
use inspectah_core::redaction::scrub_reference_userinfo;
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::FindingKind;
use inspectah_core::types::repo::parse_repo_sections;
use inspectah_core::types::rpm::{FileOwnershipEntry, KeyRejection, RepoFile, RpmVaEntry};
use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::path::Path;

/// Returns true if a repo file or its content identifies an inspectah COPR repo.
/// inspectah is the scanning tool — its repo should not appear in migration output.
///
/// Matches patterns:
/// - Filename: `_copr:copr.fedorainfracloud.org:*:inspectah.repo`
/// - Filename: `copr-*-inspectah.repo` (legacy COPR naming)
/// - Section ID in content: `[copr:copr.fedorainfracloud.org:*:inspectah]`
fn is_inspectah_repo(filename: &str, content: &str) -> bool {
    let lower_filename = filename.to_lowercase();

    // Filename-based detection
    if lower_filename.contains("inspectah") && lower_filename.ends_with(".repo") {
        return true;
    }

    // Content-based detection: look for inspectah in section IDs
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section_id = &trimmed[1..trimmed.len() - 1];
            if section_id.contains("inspectah") {
                return true;
            }
        }
    }

    false
}

/// The directory the repo files are read from.
pub(crate) const REPO_DIR: &str = "/etc/yum.repos.d";

/// The suffix a file in [`REPO_DIR`] needs for dnf, and this section, to
/// read it.
const REPO_SUFFIX: &str = ".repo";

/// Whether `path` is a file this section reads: a `*.repo` file directly
/// in [`REPO_DIR`]. The config inspector's `%ghost` branch leaves exactly
/// these to it, so a nested file or another suffix there is general
/// configuration, never dropped by both.
pub(crate) fn is_repo_file_path(path: &str) -> bool {
    path.strip_prefix(REPO_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| !name.contains('/') && name.ends_with(REPO_SUFFIX))
}

/// Collect all .repo files from /etc/yum.repos.d/
/// Excludes inspectah's own COPR repo from output. A file the read gate
/// refuses (a `.repo` name linked to a key) gives no row, and its
/// degraded reason is pushed to `left_out`.
pub fn collect_repo_files(exec: &dyn Executor, left_out: &mut Vec<String>) -> Vec<RepoFile> {
    let mut repo_files = Vec::new();
    let repo_dir = Path::new(REPO_DIR);

    let entries = match exec.read_dir(repo_dir) {
        Ok(entries) => entries,
        Err(_) => return repo_files,
    };

    for entry in entries {
        let path = repo_dir.join(&entry);
        if !is_repo_file_path(&path.to_string_lossy()) {
            continue;
        }
        let content = match exec.read_file(&path) {
            Ok(content) => content,
            Err(e) => {
                if machine_state::refusal(&e).is_some() {
                    left_out.push(machine_state::left_out_reason(&path.to_string_lossy()));
                }
                continue;
            }
        };

        // Skip files with NUL bytes or other binary content
        if content.contains('\0') {
            continue;
        }

        // Exclude inspectah's own COPR repo
        if is_inspectah_repo(&entry, &content) {
            continue;
        }

        // A file that defines no section is not a repo definition: no repo
        // owns it, and its config row, when the config walk records one,
        // stays its own control.
        if parse_repo_sections(&content).is_empty() {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();
        let is_default = is_default_repo(&path_str);

        repo_files.push(RepoFile {
            path: path_str,
            content,
            is_default_repo: is_default,
            disposition: FindingKind::from_bool(!is_default),
            locked: false,
            aggregate: None,
            gpgkey_references: Default::default(),
            shipped_by_base: None,
            verified_base_owner: None,
            redaction_changed: Default::default(),
            rejected: None,
        });
    }

    repo_files
}

/// Why a key that a repo file names was not collected. Each path is the
/// expanded path named for a diagnostic, expanded from the reference's
/// credential-free form ([`KeyRef::named`]), never one to read.
///
/// [`KeyRef::named`]: inspectah_core::types::repo::KeyRef::named
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnreadReason {
    /// Its dnf variables could not be expanded safely.
    Expand(ExpandError),
    /// The expanded path could not be read on the host.
    Unreadable(String),
    /// The file at the expanded path has no line that begins with a PGP
    /// public key header. It is recorded excluded, with its content
    /// replaced, so nothing ships it.
    NotPgp(String),
    /// The file at the expanded path holds PGP private or secret key armor,
    /// whether or not it also holds a public block. It is recorded excluded,
    /// with its content replaced, so the private material is neither stored
    /// nor shipped.
    PrivateKeyMaterial(String),
}

impl UnreadReason {
    /// What to do about it, for the warning that names it.
    pub fn remedy(&self) -> String {
        match self {
            Self::Expand(_) => "change that gpgkey= line to name a key file the image has \
                                before the package install"
                .into(),
            Self::Unreadable(path) | Self::NotPgp(path) => format!(
                "put the repo's public key in the build context and COPY it to {path} \
                 before the package install, or change that gpgkey= line"
            ),
            Self::PrivateKeyMaterial(path) => format!(
                "put a file holding only the repo's public key in the build context and \
                 COPY it to {path} before the package install, and remove the private \
                 key material from {path} on the source host"
            ),
        }
    }
}

impl fmt::Display for UnreadReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expand(e) => e.fmt(f),
            Self::Unreadable(path) => write!(f, "{path} could not be read"),
            Self::NotPgp(path) => write!(f, "{path} does not contain a PGP public key block"),
            Self::PrivateKeyMaterial(path) => write!(f, "{path} holds PGP private key material"),
        }
    }
}

/// A local key reference that produced no key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnreadGpgKey {
    /// The reference as the repo file writes it, `file://` removed, in its
    /// credential-free form ([`KeyRef::named`]): it is only ever formatted
    /// into a warning.
    ///
    /// [`KeyRef::named`]: inspectah_core::types::repo::KeyRef::named
    pub reference: String,
    pub reason: UnreadReason,
}

/// What [`extract_gpg_keys`] found in one repo file.
#[derive(Debug, Default)]
pub struct GpgKeyScan {
    pub keys: Vec<RepoFile>,
    pub unread: Vec<UnreadGpgKey>,
    /// The degraded reason for each key file the read gate refused, which
    /// is also in `unread`.
    pub left_out: Vec<String>,
}

/// Extract GPG key files referenced in repo content.
///
/// The references are each section's final `gpgkey=` value as
/// `parse_repo_sections` reads it, so a value a later assignment replaced,
/// or one before the first header, names no key.
/// Local references (`gpgkey=file:///...` or a bare path) have their dnf
/// variables expanded with the host's values, then are read and validated;
/// the key is recorded at its expanded path, which is the path dnf in the
/// image reads, with the literal path in `gpgkey_references` when the two
/// differ. Content with no line that begins with a PGP public key header,
/// or with PGP private key armor anywhere in it, is replaced with a
/// placeholder to prevent arbitrary host file ingestion.
/// Remote references (`https://...`) are left for dnf to fetch. A local
/// reference that yields no usable key is returned in `unread`.
///
/// Every diagnostic names a reference by its credential-free form
/// ([`KeyRef::named`]), taken from the raw value before the parser cut its
/// comment or split its list, and a path by that form expanded the same
/// way, never by the reference read: a password the split cut
/// (`keys:orchid92` from `file://keys:orchid92;tail@host/KEY`) holds no
/// `@`, and nothing about the piece says it is a credential.
///
/// [`KeyRef::named`]: inspectah_core::types::repo::KeyRef::named
pub fn extract_gpg_keys(repo_content: &str, exec: &dyn Executor, vars: &DnfVars) -> GpgKeyScan {
    let mut scan = GpgKeyScan::default();

    // Each section's final `gpgkey=` value, as the one parser reads it
    // (`parse_repo_sections`): ownership reads the same parse, so every
    // key recorded here is named by some section's local reference.
    let references: Vec<(String, String)> = parse_repo_sections(repo_content)
        .iter()
        .flat_map(|sec| {
            sec.local_keys_named()
                .map(|(path, named)| (path.to_owned(), named.to_owned()))
                .collect::<Vec<_>>()
        })
        .collect();
    for (reference, named) in &references {
        let reference = reference.as_str();
        // A reference that held a credential byte is named without it, so
        // a variable written inside the credential is not named either.
        let held_credential = named != reference;
        let expanded = match vars.expand(reference) {
            Ok(expanded) => expanded,
            Err(e) => {
                scan.unread.push(UnreadGpgKey {
                    reference: named.clone(),
                    reason: UnreadReason::Expand(if held_credential { e.unnamed() } else { e }),
                });
                continue;
            }
        };
        // The expanded path as a diagnostic names it: the named form
        // expanded, so a credential never enters it, and any userinfo a
        // variable's value brought in replaced.
        let shown = vars.expand(named).map_or_else(
            |_| named.clone(),
            |path| scrub_reference_userinfo(&path).into_owned(),
        );
        let key_path = expanded.as_str();
        if key_path.is_empty() {
            continue;
        }
        // The spelling `parse_repo_sections` reads back from the repo
        // file, kept so consumers can match the section to this key.
        let literal = reference;
        let gpgkey_references = if literal == key_path {
            BTreeSet::new()
        } else {
            BTreeSet::from([literal.to_string()])
        };

        let content = match exec.read_file(Path::new(key_path)) {
            Ok(content) => content,
            Err(e) => {
                if machine_state::refusal(&e).is_some() {
                    scan.left_out.push(machine_state::left_out_reason(&shown));
                }
                scan.unread.push(UnreadGpgKey {
                    reference: named.clone(),
                    reason: UnreadReason::Unreadable(shown),
                });
                continue;
            }
        };

        let rejection = match KeyRejection::of_content(&content) {
            None => {
                scan.keys.push(RepoFile {
                    path: key_path.to_string(),
                    content,
                    is_default_repo: false,
                    disposition: FindingKind::included(),
                    locked: false,
                    aggregate: None,
                    gpgkey_references,
                    shipped_by_base: None,
                    verified_base_owner: None,
                    redaction_changed: Default::default(),
                    rejected: None,
                });
                continue;
            }
            Some(rejection) => rejection,
        };
        let reason = match rejection {
            KeyRejection::NotPgp => UnreadReason::NotPgp(shown),
            KeyRejection::PrivateKeyMaterial => UnreadReason::PrivateKeyMaterial(shown),
        };
        // Include with a placeholder, so the snapshot says the key was
        // named, without leaking host file content into it.
        eprintln!("WARNING: gpgkey file {reason}, content redacted");
        scan.unread.push(UnreadGpgKey {
            reference: named.clone(),
            reason,
        });
        scan.keys.push(RepoFile {
            path: key_path.to_string(),
            content: rejection.placeholder(key_path),
            is_default_repo: false,
            disposition: FindingKind::excluded(),
            locked: false,
            aggregate: None,
            gpgkey_references,
            shipped_by_base: None,
            verified_base_owner: None,
            redaction_changed: Default::default(),
            rejected: Some(rejection),
        });
    }

    scan
}

/// Collapse keys recorded at the same path into one entry, in first-seen
/// order, with the `gpgkey_references` of every entry at that path.
///
/// Every section that names a key yields an entry for it, and RHEL's
/// `redhat.repo` names the stock key from hundreds of sections. Entries at
/// one path were read from one file, so they differ only in the
/// references that named them.
pub fn record_each_key_once(keys: Vec<RepoFile>) -> Vec<RepoFile> {
    let mut once: Vec<RepoFile> = Vec::new();
    for key in keys {
        match once.iter_mut().find(|k| k.path == key.path) {
            Some(first) => first.gpgkey_references.extend(key.gpgkey_references),
            None => once.push(key),
        }
    }
    once
}

/// Leave out of the export each key the base image already ships.
///
/// A key qualifies when the scan collected it as a valid key, a package in
/// the base image owns its path (`file_ownership`), and `rpm -Va` did not
/// report it (`rpm_va`). It is recorded excluded with that package in
/// `shipped_by_base`. Any other key, including one owned only by a package
/// the base image lacks (EPEL's key, owned by `epel-release`), is left as
/// collected, and carried.
pub fn leave_out_keys_the_base_ships(
    keys: &mut [RepoFile],
    file_ownership: &[FileOwnershipEntry],
    rpm_va: &[RpmVaEntry],
    base_packages: &HashSet<&str>,
) {
    for key in keys.iter_mut().filter(|k| k.disposition.is_included()) {
        if let Some(package) = verified_base_owner(&key.path, file_ownership, rpm_va, base_packages)
        {
            key.shipped_by_base = Some(package.to_string());
            key.disposition = FindingKind::excluded();
        }
    }
}

/// Record on each repo file the base image has its own copy of the package
/// that ships it ([`RepoFile::verified_base_owner`]), by the evidence
/// [`leave_out_keys_the_base_ships`] reads. Call it only with the report
/// of an `rpm -Va` that ran to the end. The file is still carried.
pub fn mark_repo_files_the_base_ships(
    repo_files: &mut [RepoFile],
    file_ownership: &[FileOwnershipEntry],
    rpm_va: &[RpmVaEntry],
    base_packages: &HashSet<&str>,
) {
    for file in repo_files.iter_mut() {
        file.verified_base_owner =
            verified_base_owner(&file.path, file_ownership, rpm_va, base_packages)
                .map(str::to_owned);
    }
}

/// The package the base image has that owns `path` on the host, when the
/// complete `rpm -Va` report `rpm_va` does not name `path`. The least name,
/// so a file several base packages own gives one answer whatever order rpm
/// listed them in.
fn verified_base_owner<'a>(
    path: &str,
    file_ownership: &'a [FileOwnershipEntry],
    rpm_va: &[RpmVaEntry],
    base_packages: &HashSet<&str>,
) -> Option<&'a str> {
    if rpm_va.iter().any(|e| e.path == path) {
        return None;
    }
    file_ownership
        .iter()
        .filter(|e| base_packages.contains(e.package_name.as_str()))
        .filter(|e| e.paths.iter().any(|p| p == path))
        .map(|e| e.package_name.as_str())
        .min()
}

fn is_default_repo(path: &str) -> bool {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    matches!(
        filename,
        "redhat.repo" | "rhel.repo" | "centos.repo" | "rocky.repo" | "alma.repo"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;

    #[test]
    fn test_parse_repo_files() {
        let mock = MockExecutor::new()
            .with_dir("/etc/yum.repos.d", vec!["redhat.repo", "epel.repo"])
            .with_file(
                "/etc/yum.repos.d/redhat.repo",
                "[rhel-9-baseos]\nname=RHEL 9 BaseOS\n",
            )
            .with_file(
                "/etc/yum.repos.d/epel.repo",
                "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n",
            );

        let repos = collect_repo_files(&mock, &mut Vec::new());
        assert_eq!(repos.len(), 2);

        let redhat_repo = repos.iter().find(|r| r.path.contains("redhat.repo"));
        assert!(redhat_repo.is_some());
        assert!(redhat_repo.unwrap().is_default_repo);

        let epel_repo = repos.iter().find(|r| r.path.contains("epel.repo"));
        assert!(epel_repo.is_some());
        assert!(!epel_repo.unwrap().is_default_repo);
    }

    /// The reader reads exactly what the predicate claims: a `*.repo`
    /// file directly in the directory, not one nested below it, another
    /// suffix, or a sibling directory that only starts with its name.
    #[test]
    fn the_reader_reads_exactly_the_files_the_predicate_claims() {
        for (path, claimed) in [
            ("/etc/yum.repos.d/custom.repo", true),
            ("/etc/yum.repos.d/sub/nested.repo", false),
            ("/etc/yum.repos.d/custom.repo.bak", false),
            ("/etc/yum.repos.d/repos.conf", false),
            ("/etc/yum.repos.d.bak/old.repo", false),
            ("/etc/yum.repos.d", false),
        ] {
            assert_eq!(is_repo_file_path(path), claimed, "{path}");
        }

        let mock = MockExecutor::new()
            .with_dir(
                "/etc/yum.repos.d",
                vec!["custom.repo", "sub", "custom.repo.bak", "repos.conf"],
            )
            .with_file("/etc/yum.repos.d/custom.repo", "[custom]\nname=Custom\n")
            .with_dir("/etc/yum.repos.d/sub", vec!["nested.repo"])
            .with_file("/etc/yum.repos.d/sub/nested.repo", "[nested]\nname=N\n")
            .with_file("/etc/yum.repos.d/custom.repo.bak", "[old]\nname=Old\n")
            .with_file("/etc/yum.repos.d/repos.conf", "[conf]\nname=C\n");
        let read: Vec<String> = collect_repo_files(&mock, &mut Vec::new())
            .into_iter()
            .map(|r| r.path)
            .collect();
        assert_eq!(read, ["/etc/yum.repos.d/custom.repo"]);
        assert!(read.iter().all(|path| is_repo_file_path(path)));
    }

    #[test]
    fn test_malformed_repo_file_skipped() {
        let mock = MockExecutor::new()
            .with_dir("/etc/yum.repos.d", vec!["broken.repo"])
            .with_file(
                "/etc/yum.repos.d/broken.repo",
                "not a valid repo file\n\0\0\0",
            );

        let repos = collect_repo_files(&mock, &mut Vec::new());
        // Should not panic, malformed file should be skipped
        assert_eq!(repos.len(), 0);
    }

    #[test]
    fn test_gpg_key_extraction() {
        let repo_content = "[epel]\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n";
        let mock = MockExecutor::new().with_file(
            "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9",
            "-----BEGIN PGP PUBLIC KEY BLOCK-----\n...",
        );

        let keys = extract_gpg_keys(repo_content, &mock, &DnfVars::from_host(&mock)).keys;
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].path, "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9");
        assert!(keys[0].content.starts_with("-----BEGIN PGP"));
    }

    #[test]
    fn test_gpg_key_extraction_multiple_keys() {
        let repo_content = "[r]\ngpgkey=file:///key1.asc file:///key2.asc\n";
        let mock = MockExecutor::new()
            .with_file("/key1.asc", "KEY1")
            .with_file("/key2.asc", "KEY2");

        let keys = extract_gpg_keys(repo_content, &mock, &DnfVars::from_host(&mock)).keys;
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_gpg_key_extraction_http_urls_skipped() {
        let repo_content = "[r]\ngpgkey=https://example.com/key.asc\n";
        let mock = MockExecutor::new();

        let keys = extract_gpg_keys(repo_content, &mock, &DnfVars::from_host(&mock)).keys;
        assert_eq!(keys.len(), 0);
    }

    #[test]
    fn test_gpg_key_non_pgp_content_redacted() {
        // Simulate gpgkey=file:///etc/shadow — arbitrary host file
        let repo_content = "[malicious]\ngpgkey=file:///etc/shadow\n";
        let mock =
            MockExecutor::new().with_file("/etc/shadow", "root:$6$salt$hash:19000:0:99999:7:::\n");

        let keys = extract_gpg_keys(repo_content, &mock, &DnfVars::from_host(&mock)).keys;
        assert_eq!(keys.len(), 1);
        // Content must NOT contain the shadow hash
        assert!(
            !keys[0].content.contains("$6$salt$hash"),
            "shadow file content must not be included verbatim"
        );
        // Should contain the redaction placeholder
        assert!(
            keys[0].content.contains("REDACTED"),
            "non-PGP content must be replaced with placeholder"
        );
        // Should be excluded from inclusion
        assert!(
            !keys[0].disposition.is_included(),
            "non-PGP key file must not be included in output"
        );
    }

    #[test]
    fn test_gpg_key_valid_pgp_content_included() {
        let pgp_content = "-----BEGIN PGP PUBLIC KEY BLOCK-----\nVersion: GnuPG v2\n\nmQENB...\n-----END PGP PUBLIC KEY BLOCK-----\n";
        let repo_content = "[repo]\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-test\n";
        let mock = MockExecutor::new().with_file("/etc/pki/rpm-gpg/RPM-GPG-KEY-test", pgp_content);

        let keys = extract_gpg_keys(repo_content, &mock, &DnfVars::from_host(&mock)).keys;
        assert_eq!(keys.len(), 1);
        assert!(keys[0].content.starts_with("-----BEGIN PGP"));
        assert!(
            keys[0].disposition.is_included(),
            "valid PGP key must be included"
        );
    }

    #[test]
    fn test_gpg_key_mixed_valid_and_invalid() {
        let repo_content = "[r]\ngpgkey=file:///good-key file:///bad-file\n";
        let mock = MockExecutor::new()
            .with_file("/good-key", "-----BEGIN PGP PUBLIC KEY BLOCK-----\ndata\n")
            .with_file("/bad-file", "this is not a GPG key at all");

        let keys = extract_gpg_keys(repo_content, &mock, &DnfVars::from_host(&mock)).keys;
        assert_eq!(keys.len(), 2);
        // First key is valid
        assert!(keys[0].disposition.is_included());
        assert!(keys[0].content.contains("BEGIN PGP"));
        // Second key is redacted, and marked rejected
        assert!(!keys[1].disposition.is_included());
        assert!(keys[1].content.contains("REDACTED"));
        assert_eq!(keys[0].rejected, None);
        assert_eq!(keys[1].rejected, Some(KeyRejection::NotPgp));
    }

    // --- Self-exclusion tests ---

    #[test]
    fn test_is_inspectah_repo_copr_filename() {
        // Standard COPR naming: _copr:copr.fedorainfracloud.org:user:inspectah.repo
        assert!(is_inspectah_repo(
            "_copr:copr.fedorainfracloud.org:mrussell:inspectah.repo",
            "[copr:copr.fedorainfracloud.org:mrussell:inspectah]\nbaseurl=...\n"
        ));
    }

    #[test]
    fn test_is_inspectah_repo_legacy_copr_filename() {
        // Legacy COPR naming
        assert!(is_inspectah_repo(
            "copr-mrussell-inspectah.repo",
            "[copr-mrussell-inspectah]\nbaseurl=...\n"
        ));
    }

    #[test]
    fn test_is_inspectah_repo_content_based() {
        // Filename doesn't match but content has inspectah section
        assert!(is_inspectah_repo(
            "custom-repos.repo",
            "[copr:copr.fedorainfracloud.org:conan:inspectah]\nbaseurl=...\n"
        ));
    }

    #[test]
    fn test_is_inspectah_repo_false_for_unrelated() {
        assert!(!is_inspectah_repo("epel.repo", "[epel]\nname=EPEL 9\n"));
        assert!(!is_inspectah_repo(
            "redhat.repo",
            "[rhel-9-baseos]\nname=RHEL 9 BaseOS\n"
        ));
    }

    #[test]
    fn test_collect_repo_files_excludes_inspectah_copr() {
        let mock = MockExecutor::new()
            .with_dir(
                "/etc/yum.repos.d",
                vec![
                    "redhat.repo",
                    "epel.repo",
                    "_copr:copr.fedorainfracloud.org:mrussell:inspectah.repo",
                ],
            )
            .with_file(
                "/etc/yum.repos.d/redhat.repo",
                "[rhel-9-baseos]\nname=RHEL 9 BaseOS\n",
            )
            .with_file(
                "/etc/yum.repos.d/epel.repo",
                "[epel]\nname=EPEL 9\n",
            )
            .with_file(
                "/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:mrussell:inspectah.repo",
                "[copr:copr.fedorainfracloud.org:mrussell:inspectah]\nbaseurl=https://download.copr.fedorainfracloud.org/results/mrussell/inspectah/\n",
            );

        let repos = collect_repo_files(&mock, &mut Vec::new());
        assert_eq!(repos.len(), 2, "inspectah COPR repo must be excluded");

        let repo_paths: Vec<&str> = repos.iter().map(|r| r.path.as_str()).collect();
        assert!(
            !repo_paths.iter().any(|p| p.contains("inspectah")),
            "no repo with inspectah in path should be present"
        );
    }

    /// The text before the first armored header of the stock
    /// `/etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release`, copied from
    /// `registry.redhat.io/rhel9/rhel-bootc:9.8` (header on line 10) and a
    /// RHEL 10 bootc image (header on line 12). Byte sizes match the
    /// scanned RHEL 9.8 and 10.2 hosts' files.
    const EL9_REDHAT_RELEASE_PREAMBLE: &str = "\
The following public key can be used to verify RPM packages built and
signed by Red Hat, Inc.  This key is used for packages in Red Hat
products shipped after November 2009, and for all updates to those
products.

Questions about this key should be sent to security@redhat.com.

pub  4096R/FD431D51 2009-10-22 Red Hat, Inc. (release key 2) <security@redhat.com>

";
    const EL10_REDHAT_RELEASE_PREAMBLE: &str = "\
The following public key can be used to verify RPM packages built and
signed by Red Hat, Inc.  This key is used for packages in Red Hat
products shipped after November 2009, and for all updates to those
products.

Questions about this key should be sent to security@redhat.com.

pub   4096R/FD431D51 2009-10-22
      Key fingerprint = 567E 347A D004 4ADE 55BA  8A5F 199E 2F91 FD43 1D51
uid                  Red Hat, Inc. (release key 2) <security@redhat.com>

";
    const ARMORED_BLOCK: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nmQINB...\n\
                                 -----END PGP PUBLIC KEY BLOCK-----\n";

    /// A key file whose armored header begins a line after leading text,
    /// as every stock RHEL host's Red Hat key does, is collected included
    /// and verbatim, with no warning. A header that only appears inside a
    /// line is still refused.
    #[test]
    fn a_key_with_leading_text_is_collected_when_its_header_begins_a_line() {
        const KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release";
        let repo = format!("[rhel-baseos]\ngpgkey=file://{KEY_PATH}\n");
        let el9 = format!("{EL9_REDHAT_RELEASE_PREAMBLE}{ARMORED_BLOCK}");
        let el10 = format!("{EL10_REDHAT_RELEASE_PREAMBLE}{ARMORED_BLOCK}");
        let mid_line = format!("prose {ARMORED_BLOCK}");
        let private_above = format!(
            "-----BEGIN PGP PRIVATE KEY BLOCK-----\n\nlQdGB...\n\
             -----END PGP PRIVATE KEY BLOCK-----\n{ARMORED_BLOCK}"
        );
        let secret_above = format!(
            "-----BEGIN PGP SECRET KEY BLOCK-----\n\nlQdGB...\n\
             -----END PGP SECRET KEY BLOCK-----\n{ARMORED_BLOCK}"
        );
        let private_end_only =
            format!("lQdGB...\n-----END PGP PRIVATE KEY BLOCK-----\n{ARMORED_BLOCK}");
        let not_pgp = || Some(UnreadReason::NotPgp(KEY_PATH.into()));
        let private = || Some(UnreadReason::PrivateKeyMaterial(KEY_PATH.into()));
        // `None` is collected verbatim; otherwise refused for that reason.
        let cases: [(&str, &str, Option<UnreadReason>); 6] = [
            ("RHEL 9.8 stock key", &el9, None),
            ("RHEL 10.2 stock key", &el10, None),
            ("header inside a line", &mid_line, not_pgp()),
            (
                "private block above a public block",
                &private_above,
                private(),
            ),
            (
                "secret block above a public block",
                &secret_above,
                private(),
            ),
            (
                "private END above a public block",
                &private_end_only,
                private(),
            ),
        ];
        for (name, content, refused) in cases {
            let mock = MockExecutor::new().with_file(KEY_PATH, content);
            let scan = extract_gpg_keys(&repo, &mock, &DnfVars::from_host(&mock));
            assert_eq!(scan.keys.len(), 1, "{name}");
            let stored = &scan.keys[0].content;
            assert_eq!(
                scan.keys[0].disposition.is_included(),
                refused.is_none(),
                "{name}"
            );
            match refused {
                None => {
                    assert_eq!(stored, content, "{name}: content verbatim");
                    assert!(scan.unread.is_empty(), "{name}: {:?}", scan.unread);
                }
                Some(reason) => {
                    let placeholder = match reason {
                        UnreadReason::PrivateKeyMaterial(_) => "held PGP private key material",
                        _ => "did not contain a PGP public key block",
                    };
                    assert!(
                        stored.starts_with("# REDACTED: ") && stored.contains(placeholder),
                        "{name}: {stored}"
                    );
                    assert!(
                        !stored.contains("PRIVATE KEY")
                            && !stored.contains("SECRET KEY")
                            && !stored.contains("lQdGB"),
                        "{name}: no private material stored: {stored}"
                    );
                    assert_eq!(scan.unread[0].reason, reason, "{name}");
                }
            }
        }
    }

    #[test]
    fn test_classify_gpg_content() {
        let cases = [
            ("-----BEGIN PGP PUBLIC KEY BLOCK-----\ndata", None),
            ("  \n-----BEGIN PGP PUBLIC KEY BLOCK-----\ndata", None),
            (
                "root:$6$salt$hash:19000:0:99999:7:::",
                Some(KeyRejection::NotPgp),
            ),
            ("not a key", Some(KeyRejection::NotPgp)),
            ("", Some(KeyRejection::NotPgp)),
            (
                "-----BEGIN PGP SECRET KEY BLOCK-----\n-----BEGIN PGP PUBLIC KEY BLOCK-----\n",
                Some(KeyRejection::PrivateKeyMaterial),
            ),
        ];
        for (content, expected) in cases {
            assert_eq!(KeyRejection::of_content(content), expected, "{content:?}");
        }
    }
}
