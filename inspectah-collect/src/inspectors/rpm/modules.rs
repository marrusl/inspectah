use inspectah_core::traits::executor::Executor;
use inspectah_core::types::rpm::{EnabledModuleStream, RpmVaEntry, VersionLockEntry};
use std::path::Path;

/// Parse enabled module streams from /etc/dnf/modules.d/*.module
pub fn parse_module_streams(exec: &dyn Executor) -> Vec<EnabledModuleStream> {
    let mut streams = Vec::new();
    let modules_dir = Path::new("/etc/dnf/modules.d");

    let entries = match exec.read_dir(modules_dir) {
        Ok(entries) => entries,
        Err(_) => return streams,
    };

    for entry in entries {
        if !entry.ends_with(".module") {
            continue;
        }

        let path = modules_dir.join(&entry);
        let content = match exec.read_file(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        if let Some(stream) = parse_module_file_content(&content) {
            streams.push(stream);
        }
    }

    streams
}

fn parse_module_file_content(content: &str) -> Option<EnabledModuleStream> {
    let mut module_name = String::new();
    let mut stream = String::new();
    let mut profiles = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "name" => module_name = value.to_string(),
                "stream" => stream = value.to_string(),
                "profiles" => {
                    profiles = value.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
    }

    if !module_name.is_empty() && !stream.is_empty() {
        Some(EnabledModuleStream {
            module_name,
            stream,
            profiles,
            include: true,
            baseline_match: false,
            fleet: None,
        })
    } else {
        None
    }
}

/// Parse version lock entries from versionlock configuration
pub fn parse_version_locks(exec: &dyn Executor) -> Vec<VersionLockEntry> {
    let mut locks = Vec::new();

    // Try common versionlock plugin config locations
    let config_paths = [
        "/etc/yum/pluginconf.d/versionlock.list",
        "/etc/dnf/plugins/versionlock.list",
    ];

    for config_path in &config_paths {
        let path = Path::new(config_path);
        let content = match exec.read_file(path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(entry) = parse_versionlock_line(line) {
                locks.push(entry);
            }
        }
    }

    locks
}

fn parse_versionlock_line(line: &str) -> Option<VersionLockEntry> {
    // Format: epoch:name-version-release.arch or name-epoch:version-release.arch
    let raw_pattern = line.to_string();

    // Simple NEVRA parsing - this is a simplified version
    // Real implementation might need more robust parsing
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() < 2 {
        return Some(VersionLockEntry {
            raw_pattern,
            include: true,
            ..Default::default()
        });
    }

    let epoch_str = parts[0];
    let rest = parts[1];

    let epoch = epoch_str.parse::<i32>().unwrap_or(0);

    // Parse name-version-release.arch
    let nvra_parts: Vec<&str> = rest.rsplitn(2, '.').collect();
    let arch = nvra_parts.first().map(|s| s.to_string()).unwrap_or_default();
    let nvr = nvra_parts.get(1).unwrap_or(&rest);

    let nvr_parts: Vec<&str> = nvr.rsplitn(3, '-').collect();
    let release = nvr_parts.first().map(|s| s.to_string()).unwrap_or_default();
    let version = nvr_parts.get(1).map(|s| s.to_string()).unwrap_or_default();
    let name = nvr_parts.get(2).map(|s| s.to_string()).unwrap_or_default();

    Some(VersionLockEntry {
        raw_pattern,
        name,
        epoch,
        version,
        release,
        arch,
        include: true,
        fleet: None,
    })
}

/// Parse rpm -Va output
pub fn parse_rpm_va(output: &str) -> Vec<RpmVaEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: "S.5....T.  c /path/to/file"
        // First 9 chars are flags, then optional 'c' for config file, then path
        if line.len() < 11 {
            continue;
        }

        let flags = &line[0..9];
        let rest = &line[9..].trim();

        // Check for optional 'c' marker
        let path = if let Some(stripped) = rest.strip_prefix("c ") {
            stripped.trim()
        } else {
            rest
        };

        entries.push(RpmVaEntry {
            path: path.to_string(),
            flags: flags.to_string(),
            package: None,
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;

    #[test]
    fn test_parse_module_streams() {
        let module_content = "name=nodejs\nstream=18\nprofiles=default,development\n";
        let mock = MockExecutor::new()
            .with_dir("/etc/dnf/modules.d", vec!["nodejs.module"])
            .with_file("/etc/dnf/modules.d/nodejs.module", module_content);

        let streams = parse_module_streams(&mock);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].module_name, "nodejs");
        assert_eq!(streams[0].stream, "18");
        assert_eq!(streams[0].profiles.len(), 2);
        assert!(streams[0].profiles.contains(&"default".to_string()));
    }

    #[test]
    fn test_parse_module_streams_empty_dir() {
        let mock = MockExecutor::new();
        let streams = parse_module_streams(&mock);
        assert_eq!(streams.len(), 0);
    }

    #[test]
    fn test_parse_version_locks() {
        let lock_content = "0:vim-enhanced-9.0.1592-1.el9.x86_64\n0:kernel-5.14.0-362.el9.x86_64\n";
        let mock = MockExecutor::new()
            .with_file("/etc/yum/pluginconf.d/versionlock.list", lock_content);

        let locks = parse_version_locks(&mock);
        assert_eq!(locks.len(), 2);

        let vim_lock = locks.iter().find(|l| l.name == "vim-enhanced");
        assert!(vim_lock.is_some());
        let vim = vim_lock.unwrap();
        assert_eq!(vim.version, "9.0.1592");
        assert_eq!(vim.release, "1.el9");
        assert_eq!(vim.arch, "x86_64");
        assert_eq!(vim.epoch, 0);
    }

    #[test]
    fn test_parse_rpm_va() {
        let output = "S.5....T.  c /etc/httpd/conf/httpd.conf\n..5....T.  c /etc/sysconfig/httpd\n";
        let entries = parse_rpm_va(output);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "/etc/httpd/conf/httpd.conf");
        assert_eq!(entries[0].flags, "S.5....T.");
        assert_eq!(entries[1].path, "/etc/sysconfig/httpd");
        assert_eq!(entries[1].flags, "..5....T.");
    }

    #[test]
    fn test_parse_rpm_va_without_config_marker() {
        let output = "S.5....T.  /etc/some/file\n";
        let entries = parse_rpm_va(output);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/etc/some/file");
        assert_eq!(entries[0].flags, "S.5....T.");
    }

    #[test]
    fn test_parse_rpm_va_empty() {
        let entries = parse_rpm_va("");
        assert_eq!(entries.len(), 0);
    }
}
