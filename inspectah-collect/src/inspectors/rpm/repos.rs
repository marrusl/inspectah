use inspectah_core::traits::executor::Executor;
use inspectah_core::types::rpm::RepoFile;
use std::path::Path;

/// Collect all .repo files from /etc/yum.repos.d/
pub fn collect_repo_files(exec: &dyn Executor) -> Vec<RepoFile> {
    let mut repo_files = Vec::new();
    let repo_dir = Path::new("/etc/yum.repos.d");

    let entries = match exec.read_dir(repo_dir) {
        Ok(entries) => entries,
        Err(_) => return repo_files,
    };

    for entry in entries {
        if !entry.ends_with(".repo") {
            continue;
        }

        let path = repo_dir.join(&entry);
        let content = match exec.read_file(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        // Skip files with NUL bytes or other binary content
        if content.contains('\0') {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();
        let is_default = is_default_repo(&path_str);

        repo_files.push(RepoFile {
            path: path_str,
            content,
            is_default_repo: is_default,
            include: !is_default,
            fleet: None,
        });
    }

    repo_files
}

/// Extract GPG key files referenced in repo content
pub fn extract_gpg_keys(repo_content: &str, exec: &dyn Executor) -> Vec<RepoFile> {
    let mut keys = Vec::new();

    for line in repo_content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("gpgkey=") {
            continue;
        }

        // Parse gpgkey=file:///path/to/key or gpgkey=/path/to/key
        let key_value = trimmed.strip_prefix("gpgkey=").unwrap_or("");

        // Handle multiple keys separated by space or comma
        for key_part in key_value.split(&[' ', ','][..]) {
            let key_path = key_part.trim()
                .strip_prefix("file://")
                .unwrap_or(key_part.trim());

            if key_path.is_empty() || key_path.starts_with("http://") || key_path.starts_with("https://") {
                continue;
            }

            let path = Path::new(key_path);
            let content = match exec.read_file(path) {
                Ok(content) => content,
                Err(_) => continue,
            };

            keys.push(RepoFile {
                path: key_path.to_string(),
                content,
                is_default_repo: false,
                include: true,
                fleet: None,
            });
        }
    }

    keys
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
            .with_file("/etc/yum.repos.d/redhat.repo", "[rhel-9-baseos]\nname=RHEL 9 BaseOS\n")
            .with_file("/etc/yum.repos.d/epel.repo", "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n");

        let repos = collect_repo_files(&mock);
        assert_eq!(repos.len(), 2);

        let redhat_repo = repos.iter().find(|r| r.path.contains("redhat.repo"));
        assert!(redhat_repo.is_some());
        assert!(redhat_repo.unwrap().is_default_repo);

        let epel_repo = repos.iter().find(|r| r.path.contains("epel.repo"));
        assert!(epel_repo.is_some());
        assert!(!epel_repo.unwrap().is_default_repo);
    }

    #[test]
    fn test_malformed_repo_file_skipped() {
        let mock = MockExecutor::new()
            .with_dir("/etc/yum.repos.d", vec!["broken.repo"])
            .with_file("/etc/yum.repos.d/broken.repo", "not a valid repo file\n\0\0\0");

        let repos = collect_repo_files(&mock);
        // Should not panic, malformed file should be skipped
        assert_eq!(repos.len(), 0);
    }

    #[test]
    fn test_gpg_key_extraction() {
        let repo_content = "[epel]\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n";
        let mock = MockExecutor::new()
            .with_file("/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9", "-----BEGIN PGP PUBLIC KEY BLOCK-----\n...");

        let keys = extract_gpg_keys(repo_content, &mock);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].path, "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9");
        assert!(keys[0].content.starts_with("-----BEGIN PGP"));
    }

    #[test]
    fn test_gpg_key_extraction_multiple_keys() {
        let repo_content = "gpgkey=file:///key1.asc file:///key2.asc\n";
        let mock = MockExecutor::new()
            .with_file("/key1.asc", "KEY1")
            .with_file("/key2.asc", "KEY2");

        let keys = extract_gpg_keys(repo_content, &mock);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_gpg_key_extraction_http_urls_skipped() {
        let repo_content = "gpgkey=https://example.com/key.asc\n";
        let mock = MockExecutor::new();

        let keys = extract_gpg_keys(repo_content, &mock);
        assert_eq!(keys.len(), 0);
    }
}
